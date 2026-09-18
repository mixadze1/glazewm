use wm_common::WindowState;
use wm_platform::{FocusOutline, ThreadBound};

use crate::{
  models::Container,
  traits::{CommonGetters, WindowGetters},
  user_config::UserConfig,
  wm_state::WmState,
};

pub(super) fn sync_focus_outline(
  focused: &Container,
  state: &mut WmState,
  config: &UserConfig,
) {
  let effects = &config.value.window_effects;
  let window = focused.as_window_container().ok().filter(|window| {
    effects.focused_window.border.enabled
      && effects.focused_border_width > 1
      && !state.is_paused
      && !matches!(window.state(), WindowState::Fullscreen(_))
  });
  let Some(window) = window else {
    state.focus_outline = None;
    return;
  };
  let color = effects.focused_border_color(&state.binding_modes).clone();
  let width = effects.focused_border_width;
  let native = window.native().clone();
  let result = if let Some(outline) = &mut state.focus_outline {
    outline
      .with_mut(|outline| outline.update(&native, color, width))
      .and_then(std::convert::identity)
  } else {
    state
      .dispatcher
      .dispatch_sync(|| {
        FocusOutline::new(&native, color, width).map(|outline| {
          ThreadBound::new(outline, state.dispatcher.clone())
        })
      })
      .and_then(std::convert::identity)
      .map(|outline| {
        state.focus_outline = Some(outline);
      })
  };
  if let Err(error) = result {
    state.focus_outline = None;
    tracing::warn!(%error, "Failed to update focus outline");
  }
}
