use anyhow::Context;
use wm_common::{TilingDirection, WindowState};
use wm_platform::{Direction, Rect};

use crate::{
  commands::container::{
    flatten_child_split_containers, flatten_split_container,
    move_container_within_tree, resize_tiling_container,
    set_focused_descendant, wrap_in_split_container,
  },
  models::{
    DirectionContainer, Monitor, NonTilingWindow, SplitContainer,
    TilingContainer, TilingWindow, WindowContainer,
  },
  traits::{
    CommonGetters, PositionGetters, TilingDirectionGetters,
    TilingSizeGetters, WindowGetters,
  },
  user_config::UserConfig,
  wm_state::WmState,
};

/// The distance in pixels to snap the window to the monitor's edge.
const SNAP_DISTANCE: i32 = 15;

pub fn move_window_in_direction(
  window: WindowContainer,
  direction: &Direction,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  match window {
    WindowContainer::TilingWindow(window) => {
      move_tiling_window(window, direction, state, config)
    }
    WindowContainer::NonTilingWindow(non_tiling_window) => {
      match non_tiling_window.state() {
        WindowState::Floating(_) => {
          move_floating_window(non_tiling_window, direction, state)
        }
        WindowState::Fullscreen(_) => move_to_workspace_in_direction(
          &non_tiling_window.into(),
          direction,
          state,
        ),
        _ => Ok(()),
      }
    }
  }
}

fn move_tiling_window(
  window_to_move: TilingWindow,
  direction: &Direction,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  normalize_workspace_splits(&window_to_move)?;
  // Flatten the parent split container if it only contains the window.
  while let Some(split_parent) = window_to_move
    .parent()
    .and_then(|parent| parent.as_split().cloned())
  {
    if split_parent.child_count() == 1 {
      flatten_split_container(split_parent)?;
    } else {
      break;
    }
  }

  let parent = window_to_move
    .direction_container()
    .context("No direction container.")?;

  let has_matching_tiling_direction = parent.tiling_direction()
    == TilingDirection::from_direction(direction);

  // Attempt to swap or move the window into a sibling container.
  if has_matching_tiling_direction {
    if let Some(sibling) =
      tiling_sibling_in_direction(&window_to_move, direction)
    {
      return move_to_sibling_container(
        window_to_move,
        sibling,
        direction,
        state,
      );
    }
  }

  // At the edge of an intermediate column, escape to the workspace in
  // one keypress rather than climbing each matching ancestor separately.
  if matches!(direction, Direction::Up | Direction::Down)
    && has_matching_tiling_direction
    && !parent.is_workspace()
  {
    return move_to_vertical_workspace_edge(
      window_to_move,
      direction,
      state,
      config,
    );
  }

  if matches!(direction, Direction::Up | Direction::Down)
    && !has_matching_tiling_direction
    && parent.tiling_children().count() > 1
    && (parent.tiling_children().count() > 2
      || window_to_move
        .workspace()
        .context("No workspace.")?
        .descendants()
        .filter(|c| c.is_tiling_window())
        .count()
        >= 4)
  {
    return stack_with_neighbor(window_to_move, direction, state, config);
  }

  // Attempt to move the window to workspace in given direction.
  if (has_matching_tiling_direction
    || window_to_move.tiling_siblings().count() == 0)
    && parent.is_workspace()
  {
    return move_to_workspace_in_direction(
      &window_to_move.into(),
      direction,
      state,
    );
  }

  // The window cannot be moved within the parent container, so traverse
  // upwards to find an ancestor that has the correct tiling direction.
  let target_ancestor = parent.ancestors().find_map(|ancestor| {
    ancestor.as_direction_container().ok().filter(|ancestor| {
      ancestor.tiling_direction()
        == TilingDirection::from_direction(direction)
    })
  });

  match target_ancestor {
    // If there is no suitable ancestor, then change the tiling direction
    // of the workspace.
    None => invert_workspace_tiling_direction(
      window_to_move,
      direction,
      state,
      config,
    ),
    // Otherwise, move the container into the given ancestor. This could
    // simply be the container's direct parent.
    Some(target_ancestor) => insert_into_ancestor(
      &window_to_move,
      &target_ancestor,
      direction,
      state,
    ),
  }
}

fn normalize_workspace_splits(
  window: &TilingWindow,
) -> anyhow::Result<()> {
  let workspace = window.workspace().context("No workspace.")?;
  let splits = workspace
    .descendants()
    .filter_map(|c| c.as_split().cloned())
    .collect::<Vec<_>>();
  for split in splits.into_iter().rev() {
    if let Some(parent) = split.parent() {
      if split.child_count() <= 1
        || (parent
          .as_direction_container()
          .is_ok_and(|p| p.tiling_direction() == split.tiling_direction())
          && !parent
            .as_split()
            .is_some_and(|p| p.unstack_fraction(&split.id()).is_some()))
      {
        flatten_split_container(split)?;
      }
    }
  }
  Ok(())
}

fn move_to_vertical_workspace_edge(
  window: TilingWindow,
  direction: &Direction,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let workspace = window.workspace().context("No workspace.")?;
  if workspace.tiling_direction() == TilingDirection::Horizontal {
    return invert_workspace_tiling_direction(
      window, direction, state, config,
    );
  }
  let ancestors = window.ancestors().collect::<Vec<_>>();
  let index = if *direction == Direction::Up {
    0
  } else {
    workspace.child_count()
  };
  move_container_within_tree(
    &window.clone().into(),
    &workspace.clone().into(),
    index,
    state,
  )?;
  for ancestor in ancestors {
    if ancestor.parent().is_some() {
      flatten_child_split_containers(&ancestor)?;
    }
  }
  state
    .pending_sync
    .queue_containers_to_redraw(workspace.tiling_children());
  Ok(())
}

fn stack_with_neighbor(
  window: TilingWindow,
  direction: &Direction,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let parent = window.parent().context("No parent.")?;
  let neighbor = tiling_sibling_in_direction(&window, &Direction::Right)
    .or_else(|| tiling_sibling_in_direction(&window, &Direction::Left))
    .context("No neighboring tile.")?;
  let split = SplitContainer::new(
    TilingDirection::Vertical,
    config.value.gaps.clone(),
  );
  let children = if *direction == Direction::Up {
    vec![window.clone().into(), neighbor]
  } else {
    vec![neighbor, window.clone().into()]
  };
  wrap_in_split_container(&split, &parent, &children)?;
  split.remember_unstack_shares();
  resize_tiling_container(&window.clone().into(), 0.5);
  move_container_within_tree(
    &window.clone().into(),
    &split.into(),
    window.index(),
    state,
  )?;
  state
    .pending_sync
    .queue_containers_to_redraw(parent.tiling_children());
  Ok(())
}

/// Gets the next sibling `TilingWindow` or `SplitContainer` in the given
/// direction.
fn tiling_sibling_in_direction(
  window: &TilingWindow,
  direction: &Direction,
) -> Option<TilingContainer> {
  match direction {
    Direction::Up | Direction::Left => window
      .prev_siblings()
      .find_map(|sibling| sibling.as_tiling_container().ok()),
    _ => window
      .next_siblings()
      .find_map(|sibling| sibling.as_tiling_container().ok()),
  }
}

fn move_to_sibling_container(
  window_to_move: TilingWindow,
  target_sibling: TilingContainer,
  direction: &Direction,
  state: &mut WmState,
) -> anyhow::Result<()> {
  let parent = window_to_move.parent().context("No parent.")?;

  match target_sibling {
    TilingContainer::TilingWindow(sibling_window) => {
      // Swap the window with sibling in given direction.
      move_container_within_tree(
        &window_to_move.clone().into(),
        &parent,
        sibling_window.index(),
        state,
      )?;

      state
        .pending_sync
        .queue_container_to_redraw(sibling_window)
        .queue_container_to_redraw(window_to_move);
    }
    TilingContainer::Split(sibling_split) => {
      if matches!(direction, Direction::Left | Direction::Right)
        && sibling_split.tiling_direction() == TilingDirection::Vertical
      {
        move_container_within_tree(
          &window_to_move.into(),
          &parent,
          sibling_split.index(),
          state,
        )?;
        state
          .pending_sync
          .queue_containers_to_redraw(parent.tiling_children());
        return Ok(());
      }
      let sibling_descendant =
        sibling_split.descendant_in_direction(&direction.inverse());

      // Move the window into the sibling split container.
      if let Some(sibling_descendant) = sibling_descendant {
        let target_parent = sibling_descendant
          .direction_container()
          .context("No direction container.")?;

        let has_matching_tiling_direction =
          TilingDirection::from_direction(direction)
            == target_parent.tiling_direction();

        let target_index = match direction {
          Direction::Down | Direction::Right
            if has_matching_tiling_direction =>
          {
            sibling_descendant.index()
          }
          _ => sibling_descendant.index() + 1,
        };

        move_container_within_tree(
          &window_to_move.into(),
          &target_parent.clone().into(),
          target_index,
          state,
        )?;

        state
          .pending_sync
          .queue_container_to_redraw(target_parent)
          .queue_containers_to_redraw(parent.tiling_children());
      }
    }
  }

  Ok(())
}

fn move_to_workspace_in_direction(
  window_to_move: &WindowContainer,
  direction: &Direction,
  state: &mut WmState,
) -> anyhow::Result<()> {
  let parent = window_to_move.parent().context("No parent.")?;
  let workspace = window_to_move.workspace().context("No workspace.")?;
  let monitor = parent.monitor().context("No monitor.")?;

  let target_workspace = state
    .monitor_in_direction(&monitor, direction)?
    .and_then(|monitor| monitor.displayed_workspace());

  if let Some(target_workspace) = target_workspace {
    // Since the window is crossing monitors, adjustments might need to be
    // made because of DPI.
    if monitor.has_dpi_difference(&target_workspace.clone().into())? {
      window_to_move.set_has_pending_dpi_adjustment(true);
    }

    // Update floating placement since the window has to cross monitors.
    window_to_move.set_floating_placement(
      window_to_move
        .floating_placement()
        .translate_to_center(&target_workspace.to_rect()?),
    );

    if let WindowContainer::NonTilingWindow(window_to_move) =
      &window_to_move
    {
      window_to_move.set_insertion_target(None);
    }

    let target_index = match direction {
      Direction::Down | Direction::Right => 0,
      _ => target_workspace.child_count(),
    };

    // Focus should be reassigned within the original workspace after the
    // window is moved out. For example, if the focus order is 1. tiling
    // window and 2. fullscreen window, then we'd want to retain focus on a
    // tiling window on move.
    let focus_target = state.focus_target_after_removal(window_to_move);

    move_container_within_tree(
      &window_to_move.clone().into(),
      &target_workspace.clone().into(),
      target_index,
      state,
    )?;

    if let Some(focus_target) = focus_target {
      set_focused_descendant(
        &focus_target,
        Some(&workspace.clone().into()),
      );
    }

    state
      .pending_sync
      .queue_container_to_redraw(window_to_move.clone())
      .queue_containers_to_redraw(target_workspace.tiling_children())
      .queue_containers_to_redraw(parent.tiling_children())
      .queue_cursor_jump()
      .queue_workspace_to_reorder(target_workspace);
  }

  Ok(())
}

fn invert_workspace_tiling_direction(
  window_to_move: TilingWindow,
  direction: &Direction,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let workspace = window_to_move.workspace().context("No workspace.")?;

  // Get top-level tiling children of the workspace.
  let workspace_children = workspace
    .tiling_children()
    .filter(|container| container.id() != window_to_move.id())
    .collect::<Vec<_>>();

  // Create a new split container to wrap the window's siblings. For
  // example, in the layout H[1 V[2 3]] where container 3 is moved down,
  // we create a split container around 1 and 2. This results in
  // H[H[1 V[2 3]]], and V[H[1 V[2]] 3] after the tiling direction change.
  if workspace_children.len() > 1 {
    let split_container = SplitContainer::new(
      workspace.tiling_direction(),
      config.value.gaps.clone(),
    );

    wrap_in_split_container(
      &split_container,
      &workspace.clone().into(),
      &workspace_children,
    )?;
  }

  // Invert the tiling direction of the workspace.
  workspace.set_tiling_direction(workspace.tiling_direction().inverse());

  let target_index = match direction {
    Direction::Left | Direction::Up => 0,
    _ => workspace.child_count(),
  };

  // Depending on the direction, place the window either before or after
  // the split container.
  move_container_within_tree(
    &window_to_move.clone().into(),
    &workspace.clone().into(),
    target_index,
    state,
  )?;

  // Workspace might have redundant split containers after the tiling
  // direction change. For example, V[H[1 2] 3] where container 3 is moved
  // up results in H[3 H[1 2]], and needs to be flattened to H[3 1 2].
  flatten_child_split_containers(&workspace.clone().into())?;

  // Use the same insertion share as an ordinary move into this row or
  // column. Flattening can expose more than two children; forcing 0.5
  // here made the final size depend on the route taken to this layout.
  #[allow(clippy::cast_precision_loss)]
  let target_size = 1. / workspace.tiling_children().count() as f32;
  resize_tiling_container(&window_to_move.into(), target_size);

  state
    .pending_sync
    .queue_containers_to_redraw(workspace.tiling_children());

  Ok(())
}

fn insert_into_ancestor(
  window_to_move: &TilingWindow,
  target_ancestor: &DirectionContainer,
  direction: &Direction,
  state: &mut WmState,
) -> anyhow::Result<()> {
  // Traverse upwards to find container whose parent is the target
  // ancestor. Then, depending on the direction, insert before or after
  // that container.
  let window_ancestor = window_to_move
    .ancestors()
    .find(|container| {
      container
        .parent()
        .is_some_and(|parent| parent == target_ancestor.clone().into())
    })
    .context("Window ancestor not found.")?;

  let target_index = match direction {
    Direction::Up | Direction::Left => window_ancestor.index(),
    _ => window_ancestor.index() + 1,
  };

  // Restore the saved row share when leaving our temporary stack. A new
  // equal-share insertion would inflate the neighbor on every repetition.
  let unstack_sizes = window_ancestor
    .as_split()
    .filter(|split| {
      window_to_move.parent() == Some(window_ancestor.clone())
        && split.tiling_children().count() > 1
        && split.tiling_direction() != target_ancestor.tiling_direction()
    })
    .and_then(|split| {
      let moved_size = split.tiling_size()
        * split.unstack_fraction(&window_to_move.id())?;
      let siblings = target_ancestor
        .tiling_children()
        .map(|child| {
          let size = child.tiling_size();
          (child, size)
        })
        .collect::<Vec<_>>();
      Some((moved_size, siblings))
    });

  // Move the window into the container above.
  move_container_within_tree(
    &window_to_move.clone().into(),
    &target_ancestor.clone().into(),
    target_index,
    state,
  )?;

  if let Some((moved_size, siblings)) = unstack_sizes {
    for (child, size) in siblings {
      child.set_tiling_size(if child.id() == window_ancestor.id() {
        size - moved_size
      } else {
        size
      });
    }
    window_to_move.set_tiling_size(moved_size);
  }

  state
    .pending_sync
    .queue_containers_to_redraw(target_ancestor.tiling_children());

  Ok(())
}

fn move_floating_window(
  window_to_move: NonTilingWindow,
  direction: &Direction,
  state: &mut WmState,
) -> anyhow::Result<()> {
  let new_position =
    new_floating_position(&window_to_move, direction, state)?;

  if let Some((position_rect, target_monitor)) = new_position {
    let monitor = window_to_move.monitor().context("No monitor.")?;

    // Mark window as needing DPI adjustment if it crosses monitors. The
    // handler for `PlatformEvent::LocationChanged` will update the
    // window's workspace if it goes out of bounds of its current
    // workspace.
    if monitor.id() != target_monitor.id()
      && monitor.has_dpi_difference(&target_monitor.into())?
    {
      window_to_move.set_has_pending_dpi_adjustment(true);
    }

    window_to_move.set_floating_placement(position_rect);
    state.pending_sync.queue_container_to_redraw(window_to_move);
  }

  Ok(())
}

/// Returns a tuple of the new floating position and the target monitor.
fn new_floating_position(
  window_to_move: &NonTilingWindow,
  direction: &Direction,
  state: &mut WmState,
) -> anyhow::Result<Option<(Rect, Monitor)>> {
  let monitor = window_to_move.monitor().context("No monitor.")?;
  let monitor_rect = monitor.native_properties().working_area;
  let window_pos = window_to_move.native_properties().frame;

  let is_on_monitor_edge = match direction {
    Direction::Up => window_pos.top == monitor_rect.top,
    Direction::Down => window_pos.bottom == monitor_rect.bottom,
    Direction::Left => window_pos.left == monitor_rect.left,
    Direction::Right => window_pos.right == monitor_rect.right,
  };

  // Window is on the edge of the monitor and should be moved to a
  // different monitor in the given direction.
  if is_on_monitor_edge {
    let next_monitor = state.monitor_in_direction(&monitor, direction)?;

    if let Some(next_monitor) = next_monitor {
      let monitor_rect = next_monitor.native().working_area()?.clone();

      let position = snap_to_monitor_edge(
        &window_pos,
        &monitor_rect,
        &direction.inverse(),
      )
      .clamp(&monitor_rect);

      return Ok(Some((position, next_monitor)));
    }

    return Ok(None);
  }

  let (monitor_length, window_length) = match direction {
    Direction::Up | Direction::Down => {
      (monitor_rect.height(), window_pos.height())
    }
    _ => (monitor_rect.width(), window_pos.width()),
  };

  let length_delta = monitor_length - window_length;

  // Calculate the distance the window should move based on the ratio of
  // the window's length to the monitor's length.
  #[allow(clippy::cast_precision_loss)]
  let move_distance = match window_length as f32 / monitor_length as f32 {
    x if (0.0..0.2).contains(&x) => length_delta / 5,
    x if (0.2..0.4).contains(&x) => length_delta / 4,
    x if (0.4..0.6).contains(&x) => length_delta / 3,
    _ => length_delta / 2,
  };

  // Snap the window to the current monitor's edge if it's within 15px of
  // it after the move.
  let should_snap_to_edge = match direction {
    Direction::Up => {
      window_pos.top - move_distance - SNAP_DISTANCE < monitor_rect.top
    }
    Direction::Down => {
      window_pos.bottom + move_distance + SNAP_DISTANCE
        > monitor_rect.bottom
    }
    Direction::Left => {
      window_pos.left - move_distance - SNAP_DISTANCE < monitor_rect.left
    }
    Direction::Right => {
      window_pos.right + move_distance + SNAP_DISTANCE > monitor_rect.right
    }
  };

  if should_snap_to_edge {
    let position =
      snap_to_monitor_edge(&window_pos, &monitor_rect, direction);

    return Ok(Some((position, monitor)));
  }

  // Snap the window to the current monitor's inverse edge if it's in
  // between two monitors or outside the bounds of the current monitor.
  let should_snap_to_inverse_edge = match direction {
    Direction::Up => window_pos.bottom > monitor_rect.bottom,
    Direction::Down => window_pos.top < monitor_rect.top,
    Direction::Left => window_pos.right > monitor_rect.right,
    Direction::Right => window_pos.left < monitor_rect.left,
  };

  let position = if should_snap_to_inverse_edge {
    snap_to_monitor_edge(&window_pos, &monitor_rect, &direction.inverse())
  } else {
    window_pos.translate_in_direction(direction, move_distance)
  };

  Ok(Some((position, monitor)))
}

fn snap_to_monitor_edge(
  window_pos: &Rect,
  monitor_rect: &Rect,
  edge: &Direction,
) -> Rect {
  let (x, y) = match edge {
    Direction::Up => (window_pos.x(), monitor_rect.top),
    Direction::Down => {
      (window_pos.x(), monitor_rect.bottom - window_pos.height())
    }
    Direction::Left => (monitor_rect.left, window_pos.y()),
    Direction::Right => {
      (monitor_rect.right - window_pos.width(), window_pos.y())
    }
  };

  window_pos.translate_to_coordinates(x, y)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{commands::container::attach_container, models::Workspace};

  fn move_from_nested_split(first: &Direction, last: &Direction) -> Rect {
    let outside = TilingWindow::mock().call();
    let moving = TilingWindow::mock().call();
    let neighbor = TilingWindow::mock().call();
    let split = SplitContainer::mock()
      .tiling_direction(TilingDirection::from_direction(first))
      .tiling_containers(vec![moving.clone().into(), neighbor.into()])
      .call();
    let children = if matches!(last, Direction::Right | Direction::Down) {
      vec![outside.into(), split.into()]
    } else {
      vec![split.into(), outside.into()]
    };
    let workspace = Workspace::mock()
      .tiling_direction(TilingDirection::from_direction(last))
      .tiling_containers(children)
      .call();
    let monitor = Monitor::mock().workspaces(vec![workspace]).call();
    let (event_tx, _) = tokio::sync::mpsc::unbounded_channel();
    let (exit_tx, _) = tokio::sync::mpsc::unbounded_channel();
    let (tick_tx, _) = tokio::sync::mpsc::unbounded_channel();
    let mut state = WmState::new(
      wm_platform::Dispatcher::mock(),
      event_tx,
      exit_tx,
      tick_tx,
    );
    attach_container(
      &monitor.into(),
      &state.root_container.clone().into(),
      None,
    )
    .unwrap();
    let config = UserConfig::new(Some(
      std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/assets/sample-config.yaml"),
    ))
    .unwrap();
    move_tiling_window(moving.clone(), first, &mut state, &config)
      .unwrap();
    move_tiling_window(moving.clone(), last, &mut state, &config).unwrap();
    moving.to_rect().unwrap()
  }

  #[test]
  fn placement_size_is_independent_of_previous_move_direction() {
    for (first, opposite, last) in [
      (Direction::Up, Direction::Down, Direction::Left),
      (Direction::Up, Direction::Down, Direction::Right),
      (Direction::Left, Direction::Right, Direction::Up),
      (Direction::Left, Direction::Right, Direction::Down),
    ] {
      let from_first = move_from_nested_split(&first, &last);
      let from_opposite = move_from_nested_split(&opposite, &last);
      assert_eq!(from_first, from_opposite);
    }
  }
}

#[cfg(test)]
mod tiling_regressions {
  use super::*;
  use crate::{commands::container::attach_container, models::Workspace};

  fn setup(
    children: Vec<TilingContainer>,
  ) -> (WmState, Workspace, UserConfig) {
    let (tx, _) = tokio::sync::mpsc::unbounded_channel();
    let mut state = WmState::new(
      wm_platform::Dispatcher::mock(),
      tx,
      tokio::sync::mpsc::unbounded_channel().0,
      tokio::sync::mpsc::unbounded_channel().0,
    );
    let mut config = UserConfig::new(Some(
      std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/assets/sample-config.yaml"),
    ))
    .unwrap();
    config.value.gaps = wm_common::GapsConfig::default();
    let workspace = Workspace::mock()
      .gaps_config(config.value.gaps.clone())
      .tiling_containers(children)
      .call();
    let monitor =
      Monitor::mock().workspaces(vec![workspace.clone()]).call();
    attach_container(
      &monitor.into(),
      &state.root_container.clone().into(),
      None,
    )
    .unwrap();
    state.pending_sync.clear();
    (state, workspace, config)
  }

  #[test]
  fn two_vertical_presses_reach_full_workspace_width() {
    for direction in [Direction::Up, Direction::Down] {
      for count in [3, 4, 8, 16] {
        for index in 0..count {
          let windows = (0..count)
            .map(|_| TilingWindow::mock().call())
            .collect::<Vec<_>>();
          let moving = windows[index].clone();
          let (mut state, workspace, config) =
            setup(windows.into_iter().map(Into::into).collect());
          set_focused_descendant(&moving.clone().into(), None);
          move_tiling_window(
            moving.clone(),
            &direction,
            &mut state,
            &config,
          )
          .unwrap();
          assert!(moving.parent().unwrap().is_split());
          assert!((moving.tiling_size() - 0.5).abs() < 0.00001);
          move_tiling_window(
            moving.clone(),
            &direction,
            &mut state,
            &config,
          )
          .unwrap();
          assert_eq!(moving.parent(), Some(workspace.clone().into()));
          assert_eq!(
            moving.to_rect().unwrap().width(),
            workspace.to_rect().unwrap().width()
          );
          assert!(moving.has_focus(None));
          assert!(!state.pending_sync.animations_suppressed());
        }
      }
    }
  }

  #[test]
  fn repeated_split_return_preserves_equal_and_unequal_sizes() {
    for vertical in [Direction::Up, Direction::Down] {
      for horizontal in [Direction::Left, Direction::Right] {
        for (count, unequal) in [3, 4, 8, 16]
          .into_iter()
          .flat_map(|n| [(n, false), (n, true)])
        {
          let windows = (0..count)
            .map(|_| TilingWindow::mock().call())
            .collect::<Vec<_>>();
          let moving = windows[if horizontal == Direction::Left {
            0
          } else {
            count - 1
          }]
          .clone();
          let (mut state, workspace, config) =
            setup(windows.iter().cloned().map(Into::into).collect());
          if unequal {
            for (i, window) in windows.iter().enumerate() {
              window.set_tiling_size(
                0.5 / count as f32
                  + i as f32 / (count * (count - 1)) as f32,
              );
            }
          }
          let before = windows
            .iter()
            .map(|w| w.to_rect().unwrap())
            .collect::<Vec<_>>();
          for _ in 0..20 {
            move_tiling_window(
              moving.clone(),
              &vertical,
              &mut state,
              &config,
            )
            .unwrap();
            move_tiling_window(
              moving.clone(),
              &horizontal,
              &mut state,
              &config,
            )
            .unwrap();
            normalize_workspace_splits(&moving).unwrap();
            assert_eq!(workspace.child_count(), count);
            for (window, rect) in windows.iter().zip(&before) {
              let after = window.to_rect().unwrap();
              assert!((after.width() - rect.width()).abs() <= 1);
              assert_eq!(after.height(), rect.height());
            }
          }
        }
      }
    }
  }

  #[test]
  fn return_preserves_an_existing_neighbor_column() {
    for direction in [Direction::Up, Direction::Down] {
      let a = TilingWindow::mock().call();
      let b = TilingWindow::mock().call();
      let column = SplitContainer::mock()
        .tiling_direction(TilingDirection::Vertical)
        .tiling_containers(vec![a.clone().into(), b.clone().into()])
        .call();
      let moving = TilingWindow::mock().call();
      let other = TilingWindow::mock().call();
      let (mut state, workspace, config) = setup(vec![
        other.clone().into(),
        column.into(),
        moving.clone().into(),
      ]);
      for _ in 0..20 {
        move_tiling_window(
          moving.clone(),
          &direction,
          &mut state,
          &config,
        )
        .unwrap();
        move_tiling_window(
          moving.clone(),
          &Direction::Right,
          &mut state,
          &config,
        )
        .unwrap();
        normalize_workspace_splits(&moving).unwrap();
        assert_eq!(workspace.child_count(), 3);
        assert_eq!(a.parent(), b.parent());
        assert_eq!(
          a.direction_container().unwrap().tiling_direction(),
          TilingDirection::Vertical
        );
        assert!((moving.tiling_size() - 1. / 3.).abs() < 0.00001);
        assert!((other.tiling_size() - 1. / 3.).abs() < 0.00001);
      }
    }
  }

  #[test]
  fn horizontal_arrows_do_not_insert_into_vertical_column() {
    let moving = TilingWindow::mock().call();
    let column = SplitContainer::mock()
      .tiling_direction(TilingDirection::Vertical)
      .tiling_containers(vec![
        TilingWindow::mock().call().into(),
        TilingWindow::mock().call().into(),
      ])
      .call();
    let (mut state, workspace, config) =
      setup(vec![moving.clone().into(), column.clone().into()]);
    let before = moving.to_rect().unwrap();
    for _ in 0..20 {
      move_tiling_window(
        moving.clone(),
        &Direction::Right,
        &mut state,
        &config,
      )
      .unwrap();
      assert_eq!(moving.parent(), Some(workspace.clone().into()));
      assert_eq!(column.child_count(), 2);
      assert_eq!(moving.to_rect().unwrap().height(), before.height());
      move_tiling_window(
        moving.clone(),
        &Direction::Left,
        &mut state,
        &config,
      )
      .unwrap();
      assert_eq!(moving.to_rect().unwrap(), before);
    }
  }

  #[test]
  fn nested_row_reaches_workspace_edge_in_two_moves() {
    for direction in [Direction::Up, Direction::Down] {
      let moving = TilingWindow::mock().call();
      let row = SplitContainer::mock()
        .tiling_direction(TilingDirection::Horizontal)
        .tiling_containers(vec![
          moving.clone().into(),
          TilingWindow::mock().call().into(),
        ])
        .call();
      let column = SplitContainer::mock()
        .tiling_direction(TilingDirection::Vertical)
        .tiling_containers(vec![
          row.into(),
          TilingWindow::mock().call().into(),
        ])
        .call();
      let (mut state, workspace, config) =
        setup(vec![column.into(), TilingWindow::mock().call().into()]);
      for _ in 0..2 {
        move_tiling_window(
          moving.clone(),
          &direction,
          &mut state,
          &config,
        )
        .unwrap();
      }
      assert_eq!(moving.parent(), Some(workspace.clone().into()));
      assert_eq!(
        moving.to_rect().unwrap().width(),
        workspace.to_rect().unwrap().width()
      );
    }
  }
}
