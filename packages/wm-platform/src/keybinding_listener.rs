use std::{
  collections::HashMap,
  sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
  },
};

use tokio::sync::mpsc;

use crate::{
  platform_event::KeybindingEvent, platform_impl, Dispatcher, Key,
};

/// Modifier key groups, where each entry maps a generic key (e.g.
/// `Key::Shift`) to all its variants (e.g. `Key::LShift`, `Key::RShift`).
///
/// `Cmd` and `Win` are treated as aliases within the same group.
const MODIFIER_GROUPS: &[(Key, &[Key])] = &[
  (Key::Shift, &[Key::Shift, Key::LShift, Key::RShift]),
  (Key::Ctrl, &[Key::Ctrl, Key::LCtrl, Key::RCtrl]),
  (Key::Alt, &[Key::Alt, Key::LAlt, Key::RAlt]),
  (
    Key::Win,
    &[
      Key::Win,
      Key::LWin,
      Key::RWin,
      Key::Cmd,
      Key::LCmd,
      Key::RCmd,
    ],
  ),
];

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Keybinding(Vec<Key>);

impl Keybinding {
  /// Creates a new keybinding from a vector of keys.
  ///
  /// # Errors
  ///
  /// Returns [`Error::InvalidKeybinding`] if the keybinding is empty.
  pub fn new(keys: Vec<Key>) -> crate::Result<Self> {
    if keys.is_empty() {
      return Err(crate::Error::InvalidKeybinding);
    }

    Ok(Self(keys))
  }

  /// Returns the keys in the keybinding.
  #[must_use]
  pub fn keys(&self) -> &[Key] {
    &self.0
  }

  /// Returns the trigger key in the keybinding.
  #[must_use]
  #[allow(clippy::missing_panics_doc)]
  pub fn trigger_key(&self) -> &Key {
    // SAFETY: Keys vector is verified to be non-empty in
    // `Keybinding::new`.
    self.0.last().unwrap()
  }
}

/// A listener for system-wide keybindings.
#[derive(Debug)]
pub struct KeybindingListener {
  continuous: Arc<Mutex<ContinuousBindings>>,
  /// A receiver channel for outgoing keybinding events.
  event_rx: mpsc::UnboundedReceiver<KeybindingEvent>,

  /// A map of keybindings to their trigger key.
  ///
  /// The trigger key is the final key in a keybinding. For example, in
  /// the keybinding `[Key::Cmd, Key::Shift, Key::A]`, `Key::A` is the
  /// trigger key.
  keybinding_map: Arc<Mutex<HashMap<Key, Vec<Keybinding>>>>,

  /// Whether the listener is currently enabled.
  enabled: Arc<AtomicBool>,

  /// The underlying keyboard hook used to listen for key events.
  keyboard_hook: platform_impl::KeyboardHook,
}

impl KeybindingListener {
  /// Creates an instance of `KeybindingListener`.
  pub fn new(
    keybindings: &[Keybinding],
    dispatcher: &Dispatcher,
  ) -> crate::Result<Self> {
    let (event_tx, event_rx) = mpsc::unbounded_channel();

    let keybinding_map =
      Arc::new(Mutex::new(Self::create_keybinding_map(keybindings)));

    let enabled = Arc::new(AtomicBool::new(true));
    let continuous = Arc::new(Mutex::new(ContinuousBindings::default()));

    let keyboard_hook = Self::create_keyboard_hook(
      keybinding_map.clone(),
      enabled.clone(),
      continuous.clone(),
      event_tx,
      dispatcher,
    )?;

    Ok(Self {
      continuous,
      event_rx,
      keybinding_map,
      enabled,
      keyboard_hook,
    })
  }

  /// Returns the next keybinding event from the listener.
  ///
  /// This will block until a keybinding event is available.
  pub async fn next_event(&mut self) -> Option<KeybindingEvent> {
    self.event_rx.recv().await
  }

  /// Continuous bindings expose current held state, never queued repeats.
  pub fn set_continuous_bindings(&self, bindings: Vec<Keybinding>) {
    *self.continuous.lock().unwrap() = ContinuousBindings {
      bindings,
      held: None,
    };
  }

  pub fn held_continuous_binding(&self) -> Option<Keybinding> {
    self
      .enabled
      .load(Ordering::Relaxed)
      .then(|| self.continuous.lock().unwrap().held.clone())
      .flatten()
  }

  /// Updates the keybindings for the keybinding listener.
  ///
  /// # Panics
  ///
  /// If the internal mutex is poisoned.
  pub fn update(&self, keybindings: &[Keybinding]) {
    *self.keybinding_map.lock().unwrap() =
      Self::create_keybinding_map(keybindings);
  }

  /// Enables or disables the keybinding listener.
  pub fn enable(&mut self, enabled: bool) {
    self.enabled.store(enabled, Ordering::Relaxed);
  }

  /// Terminates the keybinding listener.
  pub fn terminate(&mut self) -> crate::Result<()> {
    self.keyboard_hook.terminate()
  }

  /// Creates and starts the keyboard hook with the given callback.
  fn create_keyboard_hook(
    keybinding_map: Arc<Mutex<HashMap<Key, Vec<Keybinding>>>>,
    enabled: Arc<AtomicBool>,
    continuous: Arc<Mutex<ContinuousBindings>>,
    event_tx: mpsc::UnboundedSender<KeybindingEvent>,
    dispatcher: &Dispatcher,
  ) -> crate::Result<platform_impl::KeyboardHook> {
    platform_impl::KeyboardHook::new(
      move |event: platform_impl::KeyEvent| -> bool {
        if !event.is_keypress {
          continuous.lock().unwrap().release(event.key);
          return false;
        }
        if !enabled.load(Ordering::Relaxed) {
          return false;
        }

        let Ok(keybinding_map) = keybinding_map.lock() else {
          tracing::error!("Failed to acquire lock on keybinding map.");
          return false;
        };

        // Find keybinding candidates whose trigger key is the pressed key.
        let Some(candidates) = keybinding_map.get(&event.key) else {
          return false;
        };

        let mut cached_key_states = HashMap::new();

        // Find the matching keybindings based on the pressed keys.
        let matched_keybindings = candidates.iter().filter(|keybinding| {
          keybinding.keys().iter().all(|&key| {
            if key == event.key {
              return true;
            }

            *cached_key_states
              .entry(key)
              .or_insert_with(|| event.is_key_down(key))
          })
        });

        // Find the longest matching keybinding.
        let Some(longest_keybinding) = matched_keybindings
          .max_by_key(|keybinding| keybinding.keys().len())
        else {
          return false;
        };

        // Reject if any modifier keys not in the keybinding are held.
        let has_extra_modifiers = MODIFIER_GROUPS
          .iter()
          // Filter out modifier groups that have keys in the keybinding.
          .filter(|(_, group_keys)| {
            !group_keys
              .iter()
              .any(|key| longest_keybinding.keys().contains(key))
          })
          // Use the group's "generic" key (e.g. `Key::Shift`) to check if
          // the modifier is held. This avoids lookups for `Key::LShift`
          // and `Key::RShift`.
          .any(|(generic_key, _)| {
            cached_key_states
              .get(generic_key)
              .copied()
              .unwrap_or_else(|| event.is_key_down(*generic_key))
          });

        if has_extra_modifiers {
          return false;
        }

        if !continuous.lock().unwrap().press(longest_keybinding) {
          let _ =
            event_tx.send(KeybindingEvent(longest_keybinding.clone()));
        }

        true
      },
      dispatcher,
    )
  }

  /// Builds the keybinding map from configs.
  fn create_keybinding_map(
    keybindings: &[Keybinding],
  ) -> HashMap<Key, Vec<Keybinding>> {
    let mut keybinding_map = HashMap::new();

    for keybinding in keybindings {
      keybinding_map
        .entry(*keybinding.trigger_key())
        .or_insert_with(Vec::new)
        .push(keybinding.clone());
    }

    keybinding_map
  }
}

#[derive(Debug, Default)]
struct ContinuousBindings {
  bindings: Vec<Keybinding>,
  held: Option<Keybinding>,
}

impl ContinuousBindings {
  fn press(&mut self, binding: &Keybinding) -> bool {
    if self.bindings.contains(binding) {
      self.held = Some(binding.clone());
      true
    } else {
      self.held = None;
      false
    }
  }

  fn release(&mut self, key: Key) {
    if self
      .held
      .as_ref()
      .is_some_and(|binding| binding.keys().contains(&key))
    {
      self.held = None;
    }
  }
}

#[cfg(test)]
mod continuous_tests {
  use super::*;

  #[test]
  fn autorepeats_do_not_accumulate_and_release_stops_immediately() {
    let binding = Keybinding::new(vec![Key::Right]).unwrap();
    let mut state = ContinuousBindings {
      bindings: vec![binding.clone()],
      held: None,
    };
    for _ in 0..1000 {
      assert!(state.press(&binding));
    }
    assert_eq!(state.held, Some(binding));
    state.release(Key::Right);
    assert_eq!(state.held, None);
  }

  #[test]
  fn reversing_replaces_held_direction_and_mode_exit_cancels_it() {
    let left = Keybinding::new(vec![Key::Left]).unwrap();
    let right = Keybinding::new(vec![Key::Right]).unwrap();
    let mut state = ContinuousBindings {
      bindings: vec![left.clone(), right.clone()],
      held: None,
    };
    state.press(&left);
    state.press(&right);
    state.release(Key::Left);
    assert_eq!(state.held, Some(right));
    assert!(!state.press(&Keybinding::new(vec![Key::Escape]).unwrap()));
    assert_eq!(state.held, None);
  }
}

impl Drop for KeybindingListener {
  fn drop(&mut self) {
    let _ = self.terminate();
  }
}
