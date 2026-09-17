// Layout proportions use f32; screen pixel coordinates fit its exact
// integer range. Cursor bounds explicitly round inward before converting
// to pixels.
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]

use anyhow::Context;
use wm_common::{ResizeEdges, TilingDirection};
#[cfg(target_os = "windows")]
use wm_platform::NativeWindowWindowsExt;
use wm_platform::Rect;

use crate::{
  models::{TilingContainer, TilingWindow, WindowContainer},
  traits::{
    CommonGetters, PositionGetters, TilingDirectionGetters,
    TilingSizeGetters, WindowGetters, MIN_TILING_SIZE,
  },
  wm_state::WmState,
};

/// Query once per gesture, rather than sending messages on every mouse
/// move.
pub(super) fn cache_resize_minimums(
  window: &TilingWindow,
) -> anyhow::Result<()> {
  let workspace = window.workspace().context("No workspace.")?;
  for container in workspace.descendants() {
    let Ok(WindowContainer::TilingWindow(window)) =
      container.as_window_container()
    else {
      continue;
    };
    let current = window.to_rect()?;
    #[cfg(target_os = "windows")]
    let border_delta = window.total_border_delta()?;
    #[cfg(target_os = "windows")]
    let minimum = if window.native_properties().is_resizable {
      window
        .native()
        .minimum_tracking_size()
        .map(|(width, height)| {
          Rect::from_xy(0, 0, width, height)
            .apply_delta(&border_delta.inverse(), None)
        })
    } else {
      None
    }
    .unwrap_or(current);
    #[cfg(not(target_os = "windows"))]
    let minimum = if window.native_properties().is_resizable {
      Rect::from_xy(0, 0, 1, 1)
    } else {
      current
    };
    window.update_native_properties(|properties| {
      properties.minimum_tiling_size =
        Some((minimum.width().max(1), minimum.height().max(1)));
    });
  }
  Ok(())
}

/// Move shared dividers while keeping the opposite edges fixed. Geometry
/// produced by our own redraw equals the layout and therefore has no
/// delta.
pub(super) fn resize_tiling_window(
  window: &TilingWindow,
  frame: &Rect,
  edges: &ResizeEdges,
  state: &mut WmState,
) -> anyhow::Result<()> {
  let requested =
    frame.apply_delta(&window.border_delta().inverse(), None);
  let current = window.to_rect()?;
  if requested == current {
    return Ok(());
  }

  // A corner gesture can start with movement on just one axis. Discover
  // its second axis later without changing an already selected edge.
  let edges = extend_resize_edges(edges.clone(), &requested, &current);
  if let Some(mut drag) = window.active_drag() {
    drag.resize_edges = Some(edges.clone());
    window.set_active_drag(Some(drag));
  }

  #[cfg(target_os = "windows")]
  constrain_resize_cursor(window, &edges, state)?;

  for (enabled, horizontal, leading, delta) in [
    (edges.left, true, true, requested.left - current.left),
    (edges.right, true, false, requested.right - current.right),
    (edges.top, false, true, requested.top - current.top),
    (
      edges.bottom,
      false,
      false,
      requested.bottom - current.bottom,
    ),
  ] {
    if enabled && delta != 0 {
      if let Some(parent) =
        resize_edge(window, horizontal, leading, delta)?
      {
        state.pending_sync.queue_container_to_redraw(parent);
      }
    }
  }

  // Also restore a blocked edge (including the outside of the workspace).
  // Never animate live resizing: all affected windows must follow the
  // cursor.
  state.pending_sync.suppress_animations();
  state.pending_sync.queue_container_to_redraw(window.clone());
  Ok(())
}

fn extend_resize_edges(
  mut edges: ResizeEdges,
  requested: &Rect,
  current: &Rect,
) -> ResizeEdges {
  if !edges.left && !edges.right && requested.width() != current.width() {
    edges.left = requested.left != current.left;
    edges.right = !edges.left;
  }
  if !edges.top && !edges.bottom && requested.height() != current.height()
  {
    edges.top = requested.top != current.top;
    edges.bottom = !edges.top;
  }
  edges
}

fn resize_edge(
  window: &TilingWindow,
  horizontal: bool,
  leading: bool,
  delta: i32,
) -> anyhow::Result<Option<crate::models::Container>> {
  let mut container: TilingContainer = window.clone().into();
  while let Some(parent) = container.parent() {
    let direction = parent.as_direction_container()?;
    let same_axis = (direction.tiling_direction()
      == TilingDirection::Horizontal)
      == horizontal;
    if same_axis {
      let children = parent.tiling_children().collect::<Vec<_>>();
      let index = children
        .iter()
        .position(|child| child.id() == container.id())
        .context("Resize container missing from parent.")?;
      let neighbors = if leading {
        &children[..index]
      } else {
        &children[index + 1..]
      };
      if !neighbors.is_empty() {
        let (available, minimum, capacities) =
          resize_constraints(&container, neighbors, horizontal)?;
        if available <= 0. {
          return Ok(None);
        }
        let size = container.tiling_size();
        let capacity: f32 = capacities.iter().sum();
        let change =
          (if leading { -delta } else { delta }) as f32 / available;
        let change = change.clamp(minimum - size, capacity);
        if change == 0. {
          return Ok(None);
        }
        let neighbor_total: f32 =
          neighbors.iter().map(TilingSizeGetters::tiling_size).sum();
        container.set_tiling_size(size + change);
        for (neighbor, capacity_for_neighbor) in
          neighbors.iter().zip(capacities)
        {
          // Shrinking distributes only available space. Growing still
          // works when every neighbor is at its minimum (no
          // division by zero).
          let weight = if change > 0. {
            capacity_for_neighbor / capacity
          } else {
            neighbor.tiling_size() / neighbor_total
          };
          neighbor
            .set_tiling_size(neighbor.tiling_size() - change * weight);
        }
        return Ok(Some(parent));
      }
    }
    // No divider here: continue through nested splits to an ancestor's
    // shared boundary. Workspace edges cannot be moved.
    let Ok(ancestor) = parent.as_tiling_container() else {
      break;
    };
    container = ancestor;
  }
  Ok(None)
}

fn resize_constraints(
  container: &TilingContainer,
  neighbors: &[TilingContainer],
  horizontal: bool,
) -> anyhow::Result<(f32, f32, Vec<f32>)> {
  let parent = container.parent().context("No parent.")?;
  let (gap_x, gap_y) = container.inner_gaps()?;
  let gap = if horizontal { gap_x } else { gap_y };
  let available = length(&parent.to_rect()?, horizontal)
    - gap as f32 * container.tiling_siblings().count() as f32;
  let minimum = |child: &TilingContainer| -> anyhow::Result<f32> {
    Ok(
      ((minimum_length(child, horizontal)? + 1.) / available.max(1.))
        .max(MIN_TILING_SIZE)
        .min(child.tiling_size()),
    )
  };
  let capacities = neighbors
    .iter()
    .map(|neighbor| Ok(neighbor.tiling_size() - minimum(neighbor)?))
    .collect::<anyhow::Result<Vec<_>>>()?;
  Ok((available, minimum(container)?, capacities))
}

/// Legal displacement of a dragged edge, in layout pixels.
#[cfg(any(target_os = "windows", test))]
fn edge_limits(
  window: &TilingWindow,
  horizontal: bool,
  leading: bool,
) -> anyhow::Result<(f32, f32)> {
  let mut container: TilingContainer = window.clone().into();
  while let Some(parent) = container.parent() {
    let same_axis = (parent.as_direction_container()?.tiling_direction()
      == TilingDirection::Horizontal)
      == horizontal;
    if same_axis {
      let children = parent.tiling_children().collect::<Vec<_>>();
      let index = children
        .iter()
        .position(|child| child.id() == container.id())
        .context("Resize container missing from parent.")?;
      let neighbors = if leading {
        &children[..index]
      } else {
        &children[index + 1..]
      };
      if !neighbors.is_empty() {
        let (available, minimum, capacities) =
          resize_constraints(&container, neighbors, horizontal)?;
        let shrink =
          (container.tiling_size() - minimum) * available.max(0.);
        let grow = capacities.iter().sum::<f32>() * available.max(0.);
        return Ok(if leading {
          (-grow, shrink)
        } else {
          (-shrink, grow)
        });
      }
    }
    let Ok(ancestor) = parent.as_tiling_container() else {
      break;
    };
    container = ancestor;
  }
  Ok((0., 0.))
}

#[cfg(target_os = "windows")]
fn constrain_resize_cursor(
  window: &TilingWindow,
  edges: &ResizeEdges,
  state: &mut WmState,
) -> anyhow::Result<()> {
  let Some(drag) = window.active_drag() else {
    return Ok(());
  };
  let Some((cursor_x, cursor_y)) = drag.initial_cursor_position else {
    return Ok(());
  };
  // A late LOCATIONCHANGE after button-up must not confine the cursor
  // again.
  if !state
    .dispatcher
    .is_mouse_down(&wm_platform::MouseButton::Left)
  {
    state.resize_cursor_clip = None;
    return Ok(());
  }
  if state
    .resize_cursor_clip
    .as_ref()
    .is_some_and(|(id, _)| *id != window.id())
  {
    state.resize_cursor_clip = None;
  }
  if state.resize_cursor_clip.is_none() {
    state.resize_cursor_clip =
      Some((window.id(), wm_platform::ResizeCursorClip::new()?));
  }
  let current =
    window.to_rect()?.apply_delta(&window.border_delta(), None);
  for (enabled, horizontal, leading, cursor, initial_edge, current_edge) in [
    (
      edges.left || edges.right,
      true,
      edges.left,
      cursor_x,
      if edges.left {
        drag.initial_position.left
      } else {
        drag.initial_position.right
      },
      if edges.left {
        current.left
      } else {
        current.right
      },
    ),
    (
      edges.top || edges.bottom,
      false,
      edges.top,
      cursor_y,
      if edges.top {
        drag.initial_position.top
      } else {
        drag.initial_position.bottom
      },
      if edges.top {
        current.top
      } else {
        current.bottom
      },
    ),
  ] {
    if enabled {
      let (min, max) = edge_limits(window, horizontal, leading)?;
      let origin = cursor + current_edge - initial_edge;
      if let Some((_, clip)) = &mut state.resize_cursor_clip {
        clip.constrain_axis(
          horizontal,
          origin + min.ceil() as i32,
          origin + max.floor() as i32,
        )?;
      }
    }
  }
  Ok(())
}

fn length(rect: &Rect, horizontal: bool) -> f32 {
  (if horizontal {
    rect.width()
  } else {
    rect.height()
  }) as f32
}

/// A split must preserve every descendant's minimum with its existing
/// proportions, including gaps. A sum alone is insufficient for uneven
/// splits.
fn minimum_length(
  container: &TilingContainer,
  horizontal: bool,
) -> anyhow::Result<f32> {
  match container {
    TilingContainer::TilingWindow(window) => {
      let (width, height) = window
        .native_properties()
        .minimum_tiling_size
        .unwrap_or((1, 1));
      Ok((if horizontal { width } else { height }) as f32)
    }
    TilingContainer::Split(split) => {
      let same_axis = (split.tiling_direction()
        == TilingDirection::Horizontal)
        == horizontal;
      let children = split.tiling_children().collect::<Vec<_>>();
      let mut minimum = 0.0_f32;
      for child in &children {
        let child_minimum = minimum_length(child, horizontal)?;
        minimum = minimum.max(if same_axis {
          // One pixel protects truncation in vertical layout geometry.
          (child_minimum + 1.) / child.tiling_size().max(MIN_TILING_SIZE)
        } else {
          child_minimum
        });
      }
      if same_axis {
        let (gap_x, gap_y) = split.inner_gaps()?;
        minimum += (if horizontal { gap_x } else { gap_y }) as f32
          * children.len().saturating_sub(1) as f32;
      }
      Ok(minimum)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::{Monitor, SplitContainer, Workspace};

  fn layout(
    direction: TilingDirection,
    children: Vec<TilingContainer>,
  ) -> Monitor {
    Monitor::mock()
      .workspaces(vec![Workspace::mock()
        .tiling_direction(direction)
        .tiling_containers(children)
        .call()])
      .call()
  }

  fn window(width: i32, height: i32) -> TilingWindow {
    let window = TilingWindow::mock().call();
    window.update_native_properties(|properties| {
      properties.minimum_tiling_size = Some((width, height));
    });
    window
  }

  fn near(actual: i32, expected: i32) {
    assert!((actual - expected).abs() <= 1, "{actual} != {expected}");
  }

  #[test]
  fn corner_drag_can_start_moving_its_second_axis_later() {
    let initial = Rect::from_xy(10, 10, 400, 300);
    let edges = ResizeEdges {
      left: false,
      top: false,
      right: true,
      bottom: false,
    };
    let updated = extend_resize_edges(
      edges,
      &Rect::from_xy(10, 10, 450, 350),
      &initial,
    );
    assert!(updated.right && updated.bottom);
    assert!(!updated.left && !updated.top);
  }

  #[test]
  fn cursor_limits_match_layout_constraints_in_both_directions() {
    for horizontal in [true, false] {
      for leading in [true, false] {
        let a = window(350, 200);
        let b = window(350, 200);
        let direction = if horizontal {
          TilingDirection::Horizontal
        } else {
          TilingDirection::Vertical
        };
        let _monitor =
          layout(direction, vec![a.clone().into(), b.clone().into()]);
        let dragged = if leading { &b } else { &a };
        let (min, max) =
          edge_limits(dragged, horizontal, leading).unwrap();
        let before = dragged.to_rect().unwrap();
        let requested =
          if leading { min.ceil() } else { max.floor() } as i32;
        resize_edge(dragged, horizontal, leading, requested).unwrap();
        let after = dragged.to_rect().unwrap();
        near(
          length(&after, horizontal) as i32
            - length(&before, horizontal) as i32,
          if leading { -requested } else { requested },
        );
        let (_, remaining) =
          edge_limits(dragged, horizontal, leading).unwrap();
        if !leading {
          assert!(remaining <= 1.1);
        }
      }
    }
  }

  #[test]
  fn right_edge_moves_all_following_tiles_without_moving_left_edge() {
    let a = window(100, 100);
    let b = window(100, 100);
    let c = window(100, 100);
    let _monitor = layout(
      TilingDirection::Horizontal,
      vec![a.clone().into(), b.clone().into(), c.clone().into()],
    );
    let before = [
      a.to_rect().unwrap(),
      b.to_rect().unwrap(),
      c.to_rect().unwrap(),
    ];
    resize_edge(&a, true, false, 120).unwrap();
    let after = [
      a.to_rect().unwrap(),
      b.to_rect().unwrap(),
      c.to_rect().unwrap(),
    ];
    assert_eq!(before[0].left, after[0].left);
    near(after[0].right, before[0].right + 120);
    assert!(after[1].width() < before[1].width());
    assert!(after[2].width() < before[2].width());
    near(after[2].right, before[2].right);
    assert!(
      after[0].right <= after[1].left && after[1].right <= after[2].left
    );
  }

  #[test]
  fn left_and_top_edges_keep_the_opposite_edge_fixed() {
    for horizontal in [true, false] {
      let a = window(100, 100);
      let b = window(100, 100);
      let direction = if horizontal {
        TilingDirection::Horizontal
      } else {
        TilingDirection::Vertical
      };
      let _monitor =
        layout(direction, vec![a.clone().into(), b.clone().into()]);
      let before = b.to_rect().unwrap();
      resize_edge(&b, horizontal, true, -100).unwrap();
      let after = b.to_rect().unwrap();
      if horizontal {
        near(after.left, before.left - 100);
        near(after.right, before.right);
      } else {
        near(after.top, before.top - 100);
        near(after.bottom, before.bottom);
      }
    }
  }

  #[test]
  fn clamps_at_neighbor_minimum_and_can_reverse_from_the_limit() {
    for horizontal in [true, false] {
      let a = window(100, 100);
      let b = window(350, 250);
      let direction = if horizontal {
        TilingDirection::Horizontal
      } else {
        TilingDirection::Vertical
      };
      let _monitor =
        layout(direction, vec![a.clone().into(), b.clone().into()]);
      resize_edge(&a, horizontal, false, 10000).unwrap();
      let at_limit = b.to_rect().unwrap();
      assert!(
        length(&at_limit, horizontal)
          >= if horizontal { 350. } else { 250. }
      );
      let size = a.tiling_size();
      resize_edge(&a, horizontal, false, 10000).unwrap();
      assert!((a.tiling_size() - size).abs() < 0.00001);
      resize_edge(&a, horizontal, false, -100).unwrap();
      assert!(
        length(&b.to_rect().unwrap(), horizontal)
          > length(&at_limit, horizontal)
      );
      assert!((a.tiling_size() + b.tiling_size() - 1.).abs() < 0.00001);
    }
  }

  #[test]
  fn saturated_tiles_do_not_shrink_and_do_not_produce_nan_when_growing() {
    let a = window(100, 100);
    let b = window(10000, 10000);
    let c = window(10000, 10000);
    let _monitor = layout(
      TilingDirection::Horizontal,
      vec![a.clone().into(), b.clone().into(), c.clone().into()],
    );
    let original = a.tiling_size();
    resize_edge(&a, true, false, 200).unwrap();
    assert_eq!(a.tiling_size(), original);
    resize_edge(&a, true, false, -100).unwrap();
    for w in [&a, &b, &c] {
      assert!(w.tiling_size().is_finite());
    }
    assert!(a.tiling_size() < original);
    assert!(
      (a.tiling_size() + b.tiling_size() + c.tiling_size() - 1.).abs()
        < 0.00001
    );
  }

  #[test]
  fn can_take_space_from_other_tiles_when_one_neighbor_is_at_minimum() {
    let a = window(100, 100);
    let b = window(10000, 10000);
    let c = window(100, 100);
    let _monitor = layout(
      TilingDirection::Horizontal,
      vec![a.clone().into(), b.clone().into(), c.clone().into()],
    );
    let b_size = b.tiling_size();
    let before = a.to_rect().unwrap();
    resize_edge(&a, true, false, 100).unwrap();
    near(a.to_rect().unwrap().width(), before.width() + 100);
    assert_eq!(b.tiling_size(), b_size);
  }

  #[test]
  fn outer_edges_and_single_window_cannot_resize_the_workspace() {
    let a = window(100, 100);
    let _monitor =
      layout(TilingDirection::Horizontal, vec![a.clone().into()]);
    let original = a.to_rect().unwrap();
    for horizontal in [true, false] {
      for leading in [true, false] {
        assert!(resize_edge(&a, horizontal, leading, 100)
          .unwrap()
          .is_none());
        assert_eq!(a.to_rect().unwrap(), original);
      }
    }
  }

  #[test]
  fn nested_ancestor_resize_updates_both_rows_and_preserves_descendant_minimums(
  ) {
    let a = window(100, 100);
    let b = window(350, 100);
    let c = window(100, 100);
    let split = SplitContainer::mock()
      .tiling_direction(TilingDirection::Vertical)
      .tiling_containers(vec![b.clone().into(), c.clone().into()])
      .call();
    let _monitor = layout(
      TilingDirection::Horizontal,
      vec![a.clone().into(), split.into()],
    );
    let right = b.to_rect().unwrap().right;
    resize_edge(&c, true, true, 10000).unwrap();
    assert!(b.to_rect().unwrap().width() >= 350);
    assert_eq!(b.to_rect().unwrap().width(), c.to_rect().unwrap().width());
    near(c.to_rect().unwrap().right, right);
  }

  #[test]
  fn uneven_nested_split_keeps_each_child_above_its_minimum() {
    let a = window(100, 100);
    let b = window(180, 100);
    let c = window(100, 100);
    let split = SplitContainer::mock()
      .tiling_direction(TilingDirection::Horizontal)
      .tiling_containers(vec![b.clone().into(), c.clone().into()])
      .call();
    b.set_tiling_size(0.25);
    c.set_tiling_size(0.75);
    let _monitor = layout(
      TilingDirection::Horizontal,
      vec![a.clone().into(), split.into()],
    );
    resize_edge(&a, true, false, 10000).unwrap();
    assert!(b.to_rect().unwrap().width() >= 180);
    assert!(c.to_rect().unwrap().width() >= 100);
    assert_eq!(b.tiling_size(), 0.25);
    assert_eq!(c.tiling_size(), 0.75);
  }
}
