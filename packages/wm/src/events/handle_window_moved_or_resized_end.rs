use anyhow::Context;
use wm_common::{
  try_warn, ActiveDragOperation, FullscreenStateConfig, TilingDirection,
  WindowState,
};
use wm_platform::{LengthValue, Point, Rect};

use crate::{
  commands::{
    container::{move_container_within_tree, wrap_in_split_container},
    window::{set_window_size, update_window_state},
  },
  events::update_floating_window_position,
  models::{
    DirectionContainer, NonTilingWindow, SplitContainer, TilingContainer,
    WindowContainer,
  },
  traits::{
    CommonGetters, PositionGetters, TilingDirectionGetters, WindowGetters,
  },
  user_config::UserConfig,
  wm_state::WmState,
};

/// Handles the event for when a window is finished being moved or resized
/// by the user (e.g. via the window's drag handles).
///
/// This resizes the window if it's a tiling window and attach a dragged
/// floating window.
///
/// TODO: Move this to a better location - maybe a new `active_drag_ext`
/// mod.
pub fn handle_window_moved_or_resized_end(
  window: &WindowContainer,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let Some(active_drag) = window.active_drag() else {
    return Ok(());
  };

  state
    .window_target_positions
    .insert(window.id(), window.native_properties().frame);

  match &window {
    WindowContainer::NonTilingWindow(window) => {
      let is_maximized = try_warn!(window.native().is_maximized());

      window.update_native_properties(|properties| {
        properties.is_maximized = is_maximized;
      });

      let nearest_monitor = state
        .nearest_monitor(&window.native())
        .context("Failed to get workspace of nearest monitor.")?;

      let should_fullscreen = window.should_fullscreen(
        &nearest_monitor
          .displayed_workspace()
          .context("No workspace.")?,
      )?;

      if is_maximized || should_fullscreen {
        let fullscreen_state = if let WindowState::Fullscreen(
          fullscreen_state,
        ) = window.state()
        {
          fullscreen_state
        } else {
          config
            .value
            .window_behavior
            .state_defaults
            .fullscreen
            .clone()
        };

        let window = update_window_state(
          window.clone().into(),
          WindowState::Fullscreen(FullscreenStateConfig {
            maximized: is_maximized,
            ..fullscreen_state
          }),
          state,
          config,
        )?;

        window.set_active_drag(None);

        if is_maximized {
          // Dequeue the window from redraw if it's maximized, since the
          // window is already in the correct state.
          state
            .pending_sync
            .dequeue_container_from_redraw(window.clone());
        } else {
          // Force a redraw to snap the window to the monitor edges.
          // TODO: Skip redraw if it's already matches fullscreen frame.
          state.pending_sync.queue_container_to_redraw(window.clone());
        }

        return Ok(());
      }

      if active_drag.is_from_floating {
        update_floating_window_position(
          window,
          window.native_properties().frame,
          &nearest_monitor,
          state,
        )?;
        window.set_active_drag(None);
      } else {
        // Seed `window_target_positions` with the current drag position so
        // the snap-back animation starts from where the window actually
        // is, not its stale pre-drag tiling position.
        state
          .window_target_positions
          .insert(window.id(), window.native_properties().frame);

        // Window is a temporary floating window that should be
        // reverted back to tiling.
        let result = drop_as_tiling_window(window, state, config);
        window.set_active_drag(None);
        if let Some(container) = state.container_by_id(window.id()) {
          if let Ok(live_window) = container.as_window_container() {
            live_window.set_active_drag(None);
          }
        }
        result?;
      }
    }
    WindowContainer::TilingWindow(window) => {
      if active_drag.operation == Some(ActiveDragOperation::Move) {
        window.set_active_drag(None);
        if window.native().is_maximized()? {
          let fullscreen = update_window_state(
            window.clone().into(),
            WindowState::Fullscreen(FullscreenStateConfig {
              maximized: true,
              ..config
                .value
                .window_behavior
                .state_defaults
                .fullscreen
                .clone()
            }),
            state,
            config,
          )?;
          state.pending_sync.dequeue_container_from_redraw(fullscreen);
          return Ok(());
        }
        state.pending_sync.queue_container_to_redraw(window.clone());
        return Ok(());
      }
      tracing::info!(
        "Tiling window move/resize ended: {}",
        window.as_window_container()?
      );

      let frame = window.native_properties().frame;

      // Update the window's size based on the new frame position. This
      // means we use the actual window dimensions as the source of truth.
      set_window_size(
        window.clone().into(),
        Some(LengthValue::from_px(frame.width())),
        Some(LengthValue::from_px(frame.height())),
        state,
      )?;

      window.set_active_drag(None);

      // Force a redraw of the window to snap it back to its original
      // position. This is necessary when:
      // - The window is the only tiling window in the workspace.
      // - The window is not past the movement threshold for transitioning
      //   to floating while being dragged.
      // - Resizing in a direction that doesn't change the window's tiling
      //   size.
      state.pending_sync.queue_container_to_redraw(window.clone());
    }
  }

  Ok(())
}

/// Handles transition from temporary floating window to tiling window on
/// drag end.
#[allow(clippy::too_many_lines)]
fn drop_as_tiling_window(
  moved_window: &NonTilingWindow,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<WindowContainer> {
  tracing::info!(
    "Tiling window drag ended: {}",
    moved_window.as_window_container()?
  );

  let mouse_pos = state.dispatcher.cursor_position()?;
  let mouse_workspace = state
    .monitor_at_point(&mouse_pos)
    .and_then(|monitor| monitor.displayed_workspace())
    .or_else(|| moved_window.workspace())
    .context("Couldn't find workspace for window drop.")?;

  // Restoring tiling can flatten the tree; select targets afterwards.
  let moved_window = update_window_state(
    moved_window.clone().into(),
    WindowState::Tiling,
    state,
    config,
  )?;

  place_tiling_window(
    &moved_window,
    &mouse_pos,
    &mouse_workspace,
    state,
    config,
  )?;
  Ok(moved_window)
}

pub(crate) fn preview_tiling_drag(
  window: &WindowContainer,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let cursor = state.dispatcher.cursor_position()?;
  preview_tiling_drag_at(window, &cursor, state, config)
}

fn preview_tiling_drag_at(
  window: &WindowContainer,
  cursor: &Point,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  let Some(workspace) = state
    .monitor_at_point(&cursor)
    .and_then(|monitor| monitor.displayed_workspace())
  else {
    return Ok(());
  };
  // Hit-test the logical slots, never the animated native window frames.
  if contains_with_margin(&window.to_rect()?, &cursor, 16) {
    return Ok(());
  }
  let has_target = workspace
    .descendants()
    .filter_map(|container| container.as_window_container().ok())
    .any(|other| {
      other.id() != window.id()
        && other.state() == WindowState::Tiling
        && other
          .to_rect()
          .is_ok_and(|rect| contains_with_margin(&rect, &cursor, -16))
    });
  if !has_target && workspace.tiling_children().next().is_some() {
    return Ok(());
  }
  let previous_workspace =
    window.workspace().context("No drag workspace")?;
  place_tiling_window(window, &cursor, &workspace, state, config)?;
  state
    .pending_sync
    .queue_container_to_redraw(previous_workspace);
  state.pending_sync.queue_container_to_redraw(workspace);
  Ok(())
}

fn contains_with_margin(rect: &Rect, point: &Point, margin: i32) -> bool {
  i64::from(point.x) >= i64::from(rect.left) - i64::from(margin)
    && i64::from(point.x) <= i64::from(rect.right) + i64::from(margin)
    && i64::from(point.y) >= i64::from(rect.top) - i64::from(margin)
    && i64::from(point.y) <= i64::from(rect.bottom) + i64::from(margin)
}

fn place_tiling_window(
  moved_window: &WindowContainer,
  mouse_pos: &Point,
  mouse_workspace: &crate::models::Workspace,
  state: &mut WmState,
  config: &UserConfig,
) -> anyhow::Result<()> {
  // Get the workspace, split containers, and other windows under the
  // dragged window.
  let containers_at_pos = state
    .containers_at_point(&mouse_workspace.clone().into(), &mouse_pos)
    .into_iter()
    .filter(|container| container.id() != moved_window.id());

  // Get the deepest direction container under the dragged window.
  let target_parent: DirectionContainer = containers_at_pos
    .filter_map(|container| container.as_direction_container().ok())
    .fold(mouse_workspace.clone().into(), |acc, container| {
      if container.ancestors().count() > acc.ancestors().count() {
        container
      } else {
        acc
      }
    });

  // If the target parent has no children (i.e. an empty workspace), then
  // add the window directly.
  if !target_parent
    .tiling_children()
    .any(|child| child.id() != moved_window.id())
  {
    move_container_within_tree(
      &moved_window.clone().into(),
      &target_parent.clone().into(),
      0,
      state,
    )?;

    return Ok(());
  }

  let nearest_container = target_parent
    .children()
    .into_iter()
    .filter_map(|container| container.as_tiling_container().ok())
    .filter(|container| container.id() != moved_window.id())
    .try_fold(None, |acc: Option<TilingContainer>, container| match acc {
      Some(acc) => {
        let is_nearer = acc.to_rect()?.distance_to_point(&mouse_pos)
          < container.to_rect()?.distance_to_point(&mouse_pos);

        anyhow::Ok(Some(if is_nearer { acc } else { container }))
      }
      None => Ok(Some(container)),
    })?
    .context("No nearest container.")?;

  let tiling_direction = target_parent.tiling_direction();
  let drop_position =
    drop_position(&mouse_pos, &nearest_container.to_rect()?);

  let should_split = nearest_container.is_tiling_window()
    && match tiling_direction {
      TilingDirection::Horizontal => {
        drop_position == DropPosition::Top
          || drop_position == DropPosition::Bottom
      }
      TilingDirection::Vertical => {
        drop_position == DropPosition::Left
          || drop_position == DropPosition::Right
      }
    };

  if should_split {
    let split_container = SplitContainer::new(
      tiling_direction.inverse(),
      config.value.gaps.clone(),
    );

    wrap_in_split_container(
      &split_container,
      &target_parent.clone().into(),
      &[nearest_container],
    )?;

    let target_index = match drop_position {
      DropPosition::Top | DropPosition::Left => 0,
      _ => 1,
    };

    move_container_within_tree(
      &moved_window.clone().into(),
      &split_container.into(),
      target_index,
      state,
    )?;
  } else {
    let target_index = match drop_position {
      DropPosition::Top | DropPosition::Left => nearest_container.index(),
      _ => nearest_container.index() + 1,
    };

    let target_index = insertion_index(
      moved_window.parent() == Some(target_parent.clone().into()),
      moved_window.index(),
      target_index,
    );

    move_container_within_tree(
      &moved_window.clone().into(),
      &target_parent.clone().into(),
      target_index,
      state,
    )?;
  }

  state.pending_sync.queue_container_to_redraw(target_parent);

  Ok(())
}

fn insertion_index(
  same_parent: bool,
  source: usize,
  insertion: usize,
) -> usize {
  if same_parent && source < insertion {
    insertion - 1
  } else {
    insertion
  }
}

/// Represents where the window was dropped over another.
#[derive(Debug, Clone, PartialEq)]
enum DropPosition {
  Top,
  Bottom,
  Left,
  Right,
}

/// Gets the drop position for a window based on the mouse position.
///
/// This approach divides the window rect into an "X", creating four
/// triangular quadrants, to determine which side the cursor is closest to.
fn drop_position(mouse_pos: &Point, rect: &Rect) -> DropPosition {
  let delta_x = mouse_pos.x - rect.center_point().x;
  let delta_y = mouse_pos.y - rect.center_point().y;

  if delta_x.abs() > delta_y.abs() {
    // Window is in the left or right triangle.
    if delta_x > 0 {
      DropPosition::Right
    } else {
      DropPosition::Left
    }
  } else {
    // Window is in the top or bottom triangle.
    if delta_y > 0 {
      DropPosition::Bottom
    } else {
      DropPosition::Top
    }
  }
}

#[cfg(test)]
mod tests {
  use wm_common::ActiveDrag;
  use wm_platform::Dispatcher;

  use super::*;
  use crate::{
    commands::container::attach_container,
    models::{Monitor, TilingWindow, Workspace},
  };

  fn setup() -> (WmState, UserConfig, Workspace, Vec<WindowContainer>) {
    let windows: Vec<WindowContainer> =
      (0..3).map(|_| TilingWindow::mock().call().into()).collect();
    let workspace = Workspace::mock()
      .tiling_containers(
        windows
          .iter()
          .map(|window| window.as_tiling_container().unwrap())
          .collect(),
      )
      .call();
    let monitor =
      Monitor::mock().workspaces(vec![workspace.clone()]).call();
    let mut state = WmState::new(
      Dispatcher::mock(),
      tokio::sync::mpsc::unbounded_channel().0,
      tokio::sync::mpsc::unbounded_channel().0,
      tokio::sync::mpsc::unbounded_channel().0,
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
    state.is_focus_synced = true;
    (state, config, workspace, windows)
  }

  #[test]
  fn live_drag_reorders_before_release_and_preserves_window_size() {
    let (mut state, config, workspace, windows) = setup();
    let moved = &windows[0];
    let original_frame = moved.native_properties().frame;
    moved.set_active_drag(Some(ActiveDrag {
      operation: Some(ActiveDragOperation::Move),
      is_from_floating: false,
      initial_position: original_frame.clone(),
    }));
    let target = windows[2].to_rect().unwrap();
    let cursor = Point {
      x: target.right - 30,
      y: target.center_point().y,
    };
    preview_tiling_drag_at(moved, &cursor, &mut state, &config).unwrap();
    assert_eq!(moved.index(), 2);
    assert!(moved.active_drag().is_some());
    assert_eq!(moved.native_properties().frame, original_frame);
    for _ in 0..20 {
      preview_tiling_drag_at(moved, &cursor, &mut state, &config).unwrap();
    }
    assert_eq!(moved.index(), 2);
    assert_eq!(workspace.child_count(), 3);
    let target = windows[1].to_rect().unwrap();
    let cursor = Point {
      x: target.left + 30,
      y: target.center_point().y,
    };
    preview_tiling_drag_at(moved, &cursor, &mut state, &config).unwrap();
    assert_eq!(moved.index(), 0);
  }

  #[test]
  fn live_drag_can_create_a_vertical_split_without_repeated_nesting() {
    let (mut state, config, _, windows) = setup();
    let moved = &windows[0];
    let target = windows[1].to_rect().unwrap();
    let cursor = Point {
      x: target.center_point().x,
      y: target.top + 30,
    };
    preview_tiling_drag_at(moved, &cursor, &mut state, &config).unwrap();
    let parent = moved.parent().unwrap();
    assert!(parent.as_split().is_some());
    assert_eq!(parent.child_count(), 2);
    for _ in 0..20 {
      preview_tiling_drag_at(moved, &cursor, &mut state, &config).unwrap();
    }
    assert_eq!(moved.parent(), Some(parent));
  }

  #[test]
  fn boundary_jitter_does_not_change_the_slot() {
    let (mut state, config, _, windows) = setup();
    let moved = &windows[0];
    let slot = moved.to_rect().unwrap();
    for offset in -15..=15 {
      preview_tiling_drag_at(
        moved,
        &Point {
          x: slot.right + offset,
          y: slot.center_point().y,
        },
        &mut state,
        &config,
      )
      .unwrap();
      assert_eq!(moved.index(), 0);
    }
  }

  #[test]
  fn existing_config_enables_live_drag_and_allows_opting_out() {
    let default: wm_common::WindowBehaviorConfig =
      serde_yaml::from_str("{}").unwrap();
    assert!(default.live_drag_reordering);
    let disabled: wm_common::WindowBehaviorConfig =
      serde_yaml::from_str("live_drag_reordering: false").unwrap();
    assert!(!disabled.live_drag_reordering);
  }

  #[test]
  fn live_drag_moves_to_an_empty_monitor_and_back() {
    let (mut state, config, original, windows) = setup();
    let destination = Workspace::mock().name("2".to_string()).call();
    let monitor = Monitor::mock()
      .bounds(Rect::from_xy(1680, 0, 1680, 1050))
      .working_area(Rect::from_xy(1680, 0, 1680, 1000))
      .workspaces(vec![destination.clone()])
      .call();
    attach_container(
      &monitor.into(),
      &state.root_container.clone().into(),
      None,
    )
    .unwrap();
    let moved = &windows[0];
    preview_tiling_drag_at(
      moved,
      &Point { x: 1900, y: 400 },
      &mut state,
      &config,
    )
    .unwrap();
    assert_eq!(moved.workspace().unwrap().id(), destination.id());
    assert_eq!(destination.child_count(), 1);
    assert_eq!(original.child_count(), 2);
    let target = windows[1].to_rect().unwrap();
    preview_tiling_drag_at(
      moved,
      &Point {
        x: target.left + 30,
        y: target.center_point().y,
      },
      &mut state,
      &config,
    )
    .unwrap();
    assert_eq!(moved.workspace().unwrap().id(), original.id());
    assert_eq!(original.child_count(), 3);
    assert_eq!(destination.child_count(), 0);
  }
}
