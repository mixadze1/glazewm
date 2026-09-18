use anyhow::Context;
use wm_common::{WindowBehaviorConfig, WindowState};
use wm_platform::{LengthValue, Rect};

use crate::{
  commands::container::{
    available_tiling_length, refresh_tiling_minimums,
    refresh_window_minimum, resize_tiling_container, resize_with_minimums,
  },
  models::{NonTilingWindow, TilingWindow, WindowContainer},
  traits::{
    CommonGetters, PositionGetters, TilingSizeGetters, WindowGetters,
  },
  wm_state::WmState,
};

pub fn set_window_size(
  window: WindowContainer,
  target_width: Option<LengthValue>,
  target_height: Option<LengthValue>,
  state: &mut WmState,
  behavior: &WindowBehaviorConfig,
) -> anyhow::Result<()> {
  if state.binding_modes.iter().any(|mode| mode.name == "resize") {
    // Held keyboard input already supplies small frame-paced steps.
    // Animations would keep moving after release and cloak the border.
    state.pending_sync.suppress_animations();
  }
  match window {
    WindowContainer::TilingWindow(window) => {
      set_tiling_window_size(
        &window,
        target_width,
        target_height,
        state,
        behavior,
      )?;
    }
    WindowContainer::NonTilingWindow(window) => {
      if matches!(window.state(), WindowState::Floating(_)) {
        set_floating_window_size(
          &window,
          target_width,
          target_height,
          state,
          behavior,
        )?;
      }
    }
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::{Monitor, Workspace};

  #[test]
  fn command_resize_policy_can_bypass_native_minimums() {
    let a = TilingWindow::mock().call();
    let b = TilingWindow::mock().call();
    let workspace = Workspace::mock()
      .tiling_containers(vec![a.clone().into(), b.clone().into()])
      .call();
    let _monitor = Monitor::mock().workspaces(vec![workspace]).call();
    for window in [&a, &b] {
      window.update_native_properties(|p| {
        p.minimum_tiling_size = Some((500, 200))
      });
    }
    let (event_tx, _) = tokio::sync::mpsc::unbounded_channel();
    let (exit_tx, _) = tokio::sync::mpsc::unbounded_channel();
    let (tick_tx, _) = tokio::sync::mpsc::unbounded_channel();
    let mut state = WmState::new(
      wm_platform::Dispatcher::mock(),
      event_tx,
      exit_tx,
      tick_tx,
    );
    let mut behavior = WindowBehaviorConfig {
      resize_respects_minimum_size: true,
      ..Default::default()
    };
    set_tiling_window_length(
      &a,
      &LengthValue::from_px(100),
      true,
      &mut state,
      &behavior,
    )
    .unwrap();
    assert!(a.to_rect().unwrap().width() >= 500);
    behavior.resize_respects_minimum_size = false;
    set_tiling_window_length(
      &a,
      &LengthValue::from_px(100),
      true,
      &mut state,
      &behavior,
    )
    .unwrap();
    assert!((a.to_rect().unwrap().width() - 100).abs() <= 1);
  }

  #[test]
  fn sample_config_exposes_focus_width_and_unrestricted_command_resize() {
    let config: wm_common::ParsedConfig = serde_yaml::from_str(
      include_str!("../../../../../resources/assets/sample-config.yaml"),
    )
    .unwrap();
    assert_eq!(config.window_effects.focused_border_width, 3);
    assert!(!config.window_behavior.resize_respects_minimum_size);
    let legacy: wm_common::ParsedConfig =
      serde_yaml::from_str("{}").unwrap();
    assert_eq!(legacy.window_effects.focused_border_width, 0);
    assert!(legacy.window_effects.resize_border_color.is_none());
  }

  #[test]
  fn resize_border_color_restores_normal_color_and_supports_legacy_config()
  {
    let config: wm_common::ParsedConfig = serde_yaml::from_str(
      include_str!("../../../../../resources/assets/sample-config.yaml"),
    )
    .unwrap();
    let effects = &config.window_effects;
    let resize = config
      .binding_modes
      .iter()
      .find(|mode| mode.name == "resize")
      .unwrap()
      .clone();
    let color =
      effects.focused_border_color(std::slice::from_ref(&resize));
    assert_eq!((color.r, color.g, color.b), (255, 255, 0));
    assert!(std::ptr::eq(
      effects.focused_border_color(&[]),
      &effects.focused_window.border.color,
    ));
    let other_mode = wm_common::BindingModeConfig {
      name: "other".into(),
      ..resize.clone()
    };
    assert!(std::ptr::eq(
      effects.focused_border_color(&[other_mode]),
      &effects.focused_window.border.color,
    ));
    let legacy: wm_common::ParsedConfig =
      serde_yaml::from_str("{}").unwrap();
    assert!(std::ptr::eq(
      legacy.window_effects.focused_border_color(&[resize]),
      &legacy.window_effects.focused_window.border.color,
    ));
  }
}

fn set_tiling_window_size(
  window: &TilingWindow,
  target_width: Option<LengthValue>,
  target_height: Option<LengthValue>,
  state: &mut WmState,
  behavior: &WindowBehaviorConfig,
) -> anyhow::Result<()> {
  let workspace = window.workspace().context("No workspace.")?;
  if behavior.resize_respects_minimum_size {
    refresh_tiling_minimums(&workspace.into())?;
  }
  if let Some(target_width) = target_width {
    set_tiling_window_length(
      window,
      &target_width,
      true,
      state,
      behavior,
    )?;
  }

  if let Some(target_height) = target_height {
    set_tiling_window_length(
      window,
      &target_height,
      false,
      state,
      behavior,
    )?;
  }

  Ok(())
}

/// Updates either the width or height of a tiling window.
fn set_tiling_window_length(
  window: &TilingWindow,
  target_length: &LengthValue,
  is_width_resize: bool,
  state: &mut WmState,
  behavior: &WindowBehaviorConfig,
) -> anyhow::Result<()> {
  // When resizing a tiling window, the container to resize can actually be
  // an ancestor split container.
  let container_to_resize = window.container_to_resize(is_width_resize)?;

  if let Some(container_to_resize) = container_to_resize {
    let parent = container_to_resize.parent().context("No parent.")?;
    let parent_length =
      available_tiling_length(&container_to_resize, is_width_resize)?;
    if parent_length <= 0 {
      return Ok(());
    }

    // Convert the target length to a tiling size.
    let tiling_size = target_length.to_percentage(parent_length);

    // Skip the resize if the window is already at the target size.
    let changed = if behavior.resize_respects_minimum_size {
      resize_with_minimums(&container_to_resize, tiling_size)?
    } else {
      resize_tiling_container(&container_to_resize, tiling_size);
      true
    };
    if changed {
      state
        .pending_sync
        .queue_containers_to_redraw(parent.tiling_children());
    }
  }

  Ok(())
}

fn set_floating_window_size(
  window: &NonTilingWindow,
  target_width: Option<LengthValue>,
  target_height: Option<LengthValue>,
  state: &mut WmState,
  behavior: &WindowBehaviorConfig,
) -> anyhow::Result<()> {
  let monitor = window.monitor().context("No monitor")?;
  let monitor_rect = monitor.to_rect()?;
  let window_rect = window.to_rect()?;
  if behavior.resize_respects_minimum_size {
    refresh_window_minimum(&window.clone().into())?;
  }
  let (minimum_width, minimum_height) =
    if behavior.resize_respects_minimum_size {
      window
        .native_properties()
        .minimum_tiling_size
        .unwrap_or((1, 1))
    } else {
      (1, 1)
    };

  // Prevent resize from making the window smaller than minimum dimensions.
  // Always allow the size to be increased, even if the window would still
  // be within minimum dimension values.
  let length_with_clamp =
    |target_length: Option<i32>, current_length, min_length| {
      target_length.map_or(current_length, |target_length| {
        if target_length >= current_length {
          target_length
        } else {
          target_length.max(min_length)
        }
      })
    };

  let target_width_px = target_width
    .map(|target_width| target_width.to_px(monitor_rect.width(), None));

  let new_width =
    length_with_clamp(target_width_px, window_rect.width(), minimum_width);

  let target_height_px = target_height
    .map(|target_height| target_height.to_px(monitor_rect.height(), None));

  let new_height = length_with_clamp(
    target_height_px,
    window_rect.height(),
    minimum_height,
  );

  window.set_floating_placement(Rect::from_xy(
    window.floating_placement().x(),
    window.floating_placement().y(),
    new_width,
    new_height,
  ));

  state.pending_sync.queue_container_to_redraw(window.clone());

  Ok(())
}
