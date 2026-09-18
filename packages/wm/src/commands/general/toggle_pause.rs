use wm_common::WmEvent;
#[cfg(target_os = "windows")]
use wm_platform::NativeWindowWindowsExt;

#[cfg(target_os = "windows")]
use crate::traits::WindowGetters;
use crate::wm_state::WmState;

/// Pauses or unpauses the WM.
pub fn toggle_pause(state: &mut WmState) {
  #[cfg(target_os = "windows")]
  {
    state.resize_cursor_clip = None;
    state.focus_outline = None;
  }
  let is_paused = !state.is_paused;
  state.is_paused = is_paused;

  #[cfg(target_os = "windows")]
  if is_paused {
    // Serialize with delayed border writes so an old focus effect cannot
    // restore the border after it has been cleared.
    let mut generation = state.border_effect_generation.lock().unwrap();
    *generation = generation.wrapping_add(1);
    for window in state.windows() {
      let _ = window.native().set_border_color(None);
    }
  }

  // Redraw full container tree on unpause.
  if !is_paused {
    state.pending_sync.queue_all_effects_update();
    state
      .pending_sync
      .queue_container_to_redraw(state.root_container.clone());
  }

  state.emit_event(WmEvent::PauseChanged { is_paused });
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
  use super::*;

  #[test]
  fn pause_invalidates_delayed_borders_and_resume_restores_effects() {
    let (event_tx, _events) = tokio::sync::mpsc::unbounded_channel();
    let (exit_tx, _) = tokio::sync::mpsc::unbounded_channel();
    let (tick_tx, _) = tokio::sync::mpsc::unbounded_channel();
    let mut state = WmState::new(
      wm_platform::Dispatcher::mock(),
      event_tx,
      exit_tx,
      tick_tx,
    );
    let previous_generation =
      *state.border_effect_generation.lock().unwrap();
    toggle_pause(&mut state);
    assert!(state.is_paused);
    assert!(state.focus_outline.is_none());
    assert_ne!(
      *state.border_effect_generation.lock().unwrap(),
      previous_generation,
    );
    toggle_pause(&mut state);
    assert!(!state.is_paused);
    assert!(state.pending_sync.needs_all_effects_update());
  }
}
