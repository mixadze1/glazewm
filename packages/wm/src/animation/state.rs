use std::{
  cell::Cell,
  time::{Duration, Instant},
};

use wm_common::EasingFunction;
use wm_platform::{OpacityValue, Rect};

use crate::animation::engine::{animation_progress_at, apply_easing};

/// Residual rect travel distance, in pixels, at which a non-overshooting
/// animation completes early.
///
/// Kept below one pixel so the single-frame snap from the early-completion
/// position to the exact target is imperceptible for any travel distance.
/// Mirrors `WS_COMPLETE_THRESHOLD_PX` used by the workspace-switch driver.
const COMPLETE_THRESHOLD_PX: f32 = 1.0;

/// State of an individual window animation.
#[derive(Clone, Debug)]
pub struct WindowAnimationState {
  /// Time of the first rendered frame.
  ///
  /// Lazily initialized on the first `eased_progress_at` call so the
  /// clock starts when the first frame is actually rendered (aligned to
  /// VSync) rather than when the animation struct is created
  /// mid-`platform_sync`. Without lazy init, a cold-start gap of 1–2
  /// DWM frames causes the first rendered frame to already show
  /// non-zero progress, producing a visible jump at the start of the
  /// animation.
  start_time: Cell<Option<Instant>>,
  /// Time to wait before advancing progress.
  ///
  /// Used for staggered workspace-switch animations where each window
  /// starts at a different offset within the shared duration window.
  pub start_delay: Duration,
  pub duration: Duration,
  pub easing: EasingFunction,

  // Position animation.
  pub start_rect: Rect,
  pub target_rect: Rect,
  /// A directional exit, clipped to this monitor. The real window is
  /// revealed at target_rect only after the thumbnail has left the
  /// screen.
  workspace_flight: Option<(Rect, i32)>,
  workspace_reveal: Option<(Duration, EasingFunction)>,

  // Opacity animation; `None` when fade is disabled.
  pub start_opacity: Option<OpacityValue>,
  pub target_opacity: Option<OpacityValue>,
}

impl WindowAnimationState {
  /// Creates a new movement animation.
  pub fn new_movement(
    start_rect: Rect,
    target_rect: Rect,
    duration_ms: u32,
    easing: EasingFunction,
  ) -> Self {
    Self {
      start_time: Cell::new(None),
      start_delay: Duration::ZERO,
      duration: Duration::from_millis(u64::from(duration_ms)),
      easing,
      start_rect,
      target_rect,
      workspace_flight: None,
      workspace_reveal: None,
      start_opacity: None,
      target_opacity: None,
    }
  }

  /// Adds an exit-only flight toward the selected workspace.
  pub fn set_workspace_flight(&mut self, monitor: Rect, direction: i32) {
    if direction != 0 {
      self.workspace_flight = Some((monitor, direction.signum()));
    }
  }

  pub fn set_workspace_reveal(
    &mut self,
    duration_ms: u32,
    easing: EasingFunction,
  ) {
    if self.workspace_flight.is_some() && duration_ms > 0 {
      let duration = Duration::from_millis(u64::from(duration_ms));
      self.duration += duration;
      self.workspace_reveal = Some((duration, easing));
    }
  }

  fn reveal_split(&self) -> Option<f32> {
    let (duration, _) = self.workspace_reveal.as_ref()?;
    Some(
      (self.duration - *duration).as_secs_f32()
        / self.duration.as_secs_f32(),
    )
  }

  pub fn workspace_reveal_progress_at(&self, now: Instant) -> Option<f32> {
    let split = self.reveal_split()?;
    let progress = self.eased_progress_at(now);
    let (reveal_duration, _) = self.workspace_reveal.as_ref()?;
    let elapsed = now
      .saturating_duration_since(self.start_time.get()?)
      .saturating_sub(self.start_delay);
    if elapsed < self.duration - *reveal_duration {
      return None;
    }
    (progress >= split)
      .then(|| ((progress - split) / (1.0 - split)).clamp(0.0, 1.0))
  }

  fn flight_exit_rect(&self) -> Option<Rect> {
    let (monitor, direction) = self.workspace_flight.as_ref()?;
    let x = if *direction < 0 {
      monitor.x() - self.start_rect.width()
    } else {
      monitor.x() + monitor.width()
    };
    Some(Rect::from_xy(
      x,
      self.start_rect.y(),
      self.start_rect.width(),
      self.start_rect.height(),
    ))
  }

  fn rect_at_progress(&self, progress: f32) -> Rect {
    if let Some(split) = self.reveal_split() {
      if progress >= split {
        let scale = ((progress - split) / (1.0 - split)).clamp(0.0, 1.0);
        let width =
          (self.target_rect.width() as f32 * scale).round() as i32;
        let height =
          (self.target_rect.height() as f32 * scale).round() as i32;
        return Rect::from_xy(
          self.target_rect.x() + (self.target_rect.width() - width) / 2,
          self.target_rect.y() + (self.target_rect.height() - height) / 2,
          width,
          height,
        );
      }
      return self
        .start_rect
        .interpolate(&self.flight_exit_rect().unwrap(), progress / split);
    }
    if let Some(exit) = self.flight_exit_rect() {
      self.start_rect.interpolate(&exit, progress.clamp(0.0, 1.0))
    } else {
      self.start_rect.interpolate(&self.target_rect, progress)
    }
  }

  /// Sets the delay before this animation starts and returns `self`.
  #[allow(dead_code)]
  pub fn with_delay(mut self, delay: Duration) -> Self {
    self.start_delay = delay;
    self
  }

  /// Gets the eased progress in [0.0, 1.0] at an explicit `now` instant.
  ///
  /// Allows callers to supply a predictive timestamp (e.g. vsync wake-up
  /// time plus an estimated pipeline offset) so the computed position
  /// aligns with the DWM composition event rather than the moment this
  /// code runs.
  ///
  /// `start_delay` is applied before the duration window begins: if
  /// `elapsed < start_delay`, returns 0.0 without advancing the animation.
  /// All windows initialized on the same tick share the same `start_time`,
  /// so staggering is purely a function of each window's `start_delay`.
  ///
  /// Non-overshooting curves snap to 1.0 at 99% eased progress to avoid
  /// the "stuck at destination" look. Overshooting curves run to full
  /// wall-clock duration to preserve their bounce.
  pub fn eased_progress_at(&self, now: Instant) -> f32 {
    let start = self.start_time.get().unwrap_or_else(|| {
      self.start_time.set(Some(now));
      now
    });

    let elapsed = now.saturating_duration_since(start);
    if elapsed < self.start_delay {
      return 0.0;
    }

    // Shift the clock origin past the delay so the duration window begins
    // at `start + start_delay`. `animation_progress_at` uses
    // `saturating_duration_since`, so passing a future `effective_start`
    // is safe even if `now` precedes it on the first delayed tick.
    let effective_start = start + self.start_delay;
    let raw = animation_progress_at(effective_start, self.duration, now);
    if let Some((_, reveal_easing)) = &self.workspace_reveal {
      let split = self.reveal_split().unwrap();
      return if raw < split {
        split * apply_easing(raw / split, &self.easing).clamp(0.0, 1.0)
      } else {
        split
          + (1.0 - split)
            * apply_easing((raw - split) / (1.0 - split), reveal_easing)
              .clamp(0.0, 1.0)
      };
    }
    let eased = apply_easing(raw, &self.easing);
    let done = if self.easing.can_overshoot() {
      raw == 1.0
    } else if raw == 1.0 {
      true
    } else {
      // Decelerating easing spends a large fraction of its wall-clock
      // duration covering the final sliver of distance, which looks
      // "stuck" at the destination — so complete early. Gating that
      // completion on a fixed residual *pixel* distance (rather than
      // a fixed eased fraction) keeps the completion-frame snap
      // sub-pixel regardless of travel distance: a fixed `eased >=
      // 0.99` would snap ~1% of the travel, which is imperceptible
      // for a short move but a visible 10-20px jump at the end of an
      // open/close slide spanning a whole window dimension. Mirrors
      // `WS_COMPLETE_THRESHOLD_PX` in the workspace-switch driver.
      let max_travel = self.max_travel_px();
      if max_travel > 0.0 {
        (1.0 - eased) * max_travel <= COMPLETE_THRESHOLD_PX
      } else {
        // No positional travel (e.g. an opacity-only fade): there is no
        // pixel distance to gate on, so fall back to the eased fraction.
        eased >= 0.99
      }
    };
    if done {
      1.0
    } else {
      eased
    }
  }

  /// Gets the eased progress in [0.0, 1.0], snapping to 1.0 when complete.
  pub fn eased_progress(&self) -> f32 {
    self.eased_progress_at(Instant::now())
  }

  /// Remaining wall-clock time until the animation's duration window
  /// elapses at `now`.
  ///
  /// Returns the full `start_delay + duration` when the animation has not
  /// rendered its first frame yet (`start_time` unset). Used to schedule
  /// the mid-animation handoff of the real window to its final rect.
  pub fn remaining_at(&self, now: Instant) -> Duration {
    match self.start_time.get() {
      Some(start) => (start + self.start_delay + self.duration)
        .saturating_duration_since(now),
      None => self.start_delay + self.duration,
    }
  }

  /// Largest per-axis rect travel distance, in pixels, between the start
  /// and target rects.
  ///
  /// Returns the maximum of the absolute position and size deltas, giving
  /// an upper bound on how far any edge of the window moves over the
  /// animation. Returns `0` for opacity-only animations whose start and
  /// target rects are identical.
  fn max_travel_px(&self) -> f32 {
    if let Some(exit) = self.flight_exit_rect() {
      return (exit.x() - self.start_rect.x()).abs() as f32;
    }
    let dx = (self.target_rect.x() - self.start_rect.x()).abs();
    let dy = (self.target_rect.y() - self.start_rect.y()).abs();
    let dw = (self.target_rect.width() - self.start_rect.width()).abs();
    let dh = (self.target_rect.height() - self.start_rect.height()).abs();
    #[allow(clippy::cast_precision_loss)]
    {
      dx.max(dy).max(dw).max(dh) as f32
    }
  }

  /// Whether the animation has completed.
  pub fn is_complete(&self) -> bool {
    self.eased_progress() == 1.0
  }

  /// Gets the interpolated rect at the current animation progress.
  pub fn current_rect(&self) -> Rect {
    self.rect_at_progress(self.eased_progress())
  }

  /// Gets the interpolated rect and opacity in a single call.
  ///
  /// Prefer this over separate `current_rect` + `current_opacity` calls
  /// when both values are needed in the same frame — `eased_progress`
  /// (which runs a Newton-Raphson solve) is computed only once.
  pub fn current_state(&self) -> (Rect, Option<OpacityValue>) {
    self.current_state_at(Instant::now())
  }

  /// Gets the interpolated rect and opacity at an explicit `now` instant.
  ///
  /// Like [`current_state`], but evaluates progress at a caller-supplied
  /// predictive timestamp (e.g. a vsync wake-up led forward by a fraction
  /// of a frame) so the computed position aligns with the next DWM
  /// composition rather than the moment this code runs.
  ///
  /// [`current_state`]: WindowAnimationState::current_state
  pub fn current_state_at(
    &self,
    now: Instant,
  ) -> (Rect, Option<OpacityValue>) {
    let progress = self.eased_progress_at(now);
    let rect = self.rect_at_progress(progress);
    let opacity = match (&self.start_opacity, &self.target_opacity) {
      (Some(start), Some(end)) => Some(start.interpolate(end, progress)),
      _ => None,
    };
    (rect, opacity)
  }
}

#[cfg(test)]
mod tests {
  use wm_platform::Rect;

  use super::*;

  #[test]
  fn workspace_reveal_starts_only_after_exit_and_finishes_at_target() {
    let start = Rect::from_xy(100, 100, 640, 480);
    let target = Rect::from_xy(900, 200, 800, 600);
    let mut animation = WindowAnimationState::new_movement(
      start.clone(),
      target.clone(),
      450,
      linear(),
    );
    animation.set_workspace_flight(Rect::from_xy(0, 0, 1920, 1080), -1);
    animation.set_workspace_reveal(250, linear());
    let now = Instant::now();
    assert_eq!(animation.current_state_at(now).0, start);
    assert!(animation
      .workspace_reveal_progress_at(now + Duration::from_millis(225))
      .is_none());
    assert!(
      animation
        .current_state_at(now + Duration::from_millis(225))
        .0
        .x()
        < start.x()
    );
    let boundary = now + Duration::from_millis(450);
    assert!(
      animation.workspace_reveal_progress_at(boundary).unwrap() < 0.0001
    );
    assert_eq!(animation.current_state_at(boundary).0.width(), 0);
    let halfway = animation
      .current_state_at(now + Duration::from_millis(575))
      .0;
    assert!((halfway.width() - 400).abs() <= 1);
    assert!((halfway.height() - 300).abs() <= 1);
    assert!(
      (halfway.x() + halfway.width() / 2
        - (target.x() + target.width() / 2))
        .abs()
        <= 1
    );
    assert_eq!(
      animation
        .current_state_at(now + Duration::from_millis(700))
        .0,
      target
    );
  }

  #[test]
  fn zero_exit_duration_can_reveal_without_division_by_zero() {
    let rect = Rect::from_xy(100, 100, 640, 480);
    let mut animation = WindowAnimationState::new_movement(
      rect.clone(),
      rect.clone(),
      0,
      linear(),
    );
    animation.set_workspace_flight(Rect::from_xy(0, 0, 1920, 1080), 1);
    animation.set_workspace_reveal(250, linear());
    let now = Instant::now();
    assert_eq!(animation.workspace_reveal_progress_at(now), Some(0.0));
    assert_eq!(
      animation
        .current_state_at(now + Duration::from_millis(250))
        .0,
      rect
    );
  }

  #[test]
  fn workspace_flights_only_exit_without_reentering() {
    for monitor_x in [-1920, 0, 1920] {
      let monitor = Rect::from_xy(monitor_x, 0, 1920, 1080);
      let start = Rect::from_xy(monitor_x + 100, 100, 640, 480);
      let target = Rect::from_xy(monitor_x + 900, 200, 800, 600);
      for direction in [-1, 1] {
        let mut animation = WindowAnimationState::new_movement(
          start.clone(),
          target.clone(),
          450,
          EasingFunction::default(),
        );
        animation.set_workspace_flight(monitor.clone(), direction);
        assert_eq!(animation.rect_at_progress(0.0), start);
        let mut previous_x = start.x();
        for frame in 1..=100 {
          let rect = animation.rect_at_progress(frame as f32 / 100.0);
          assert!((rect.x() - previous_x) * direction >= 0);
          assert_eq!(rect.y(), start.y());
          assert_eq!(rect.width(), start.width());
          assert_eq!(rect.height(), start.height());
          previous_x = rect.x();
        }
        let exit = animation.rect_at_progress(1.0);
        assert!(exit.right <= monitor.left || exit.left >= monitor.right);
        // Native-window handoff still uses the destination layout.
        assert_eq!(animation.target_rect, target);
      }
    }
  }

  #[test]
  fn same_layout_still_flies_instead_of_completing_immediately() {
    let rect = Rect::from_xy(100, 100, 640, 480);
    let mut animation = WindowAnimationState::new_movement(
      rect.clone(),
      rect.clone(),
      450,
      EasingFunction::default(),
    );
    animation.set_workspace_flight(Rect::from_xy(0, 0, 1920, 1080), -1);
    let now = Instant::now();
    assert_eq!(animation.eased_progress_at(now), 0.0);
    assert_eq!(animation.max_travel_px(), 740.0);
    assert_ne!(animation.rect_at_progress(0.25), rect);
    assert_eq!(
      animation
        .current_state_at(now + Duration::from_millis(450))
        .0,
      Rect::from_xy(-640, 100, 640, 480)
    );
  }

  /// A cubic bezier with collinear, evenly-spaced control points is
  /// exactly the identity curve, so `eased == raw`. Used to make
  /// completion behaviour deterministic in tests.
  fn linear() -> EasingFunction {
    EasingFunction::CubicBezier(1.0 / 3.0, 1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0)
  }

  /// A long slide must not snap to the target while it is still many
  /// pixels away: at 99% progress over a 10000px slide the residual is
  /// 100px, so the animation reports the eased value rather than
  /// completing.
  #[test]
  fn long_slide_does_not_snap_at_ninety_nine_percent() {
    let anim = WindowAnimationState::new_movement(
      Rect::from_xy(0, 0, 100, 100),
      Rect::from_xy(10_000, 0, 100, 100),
      10_000,
      linear(),
    );

    let t0 = Instant::now();
    // First call anchors `start_time` at `t0`.
    assert_eq!(anim.eased_progress_at(t0), 0.0);

    let progress =
      anim.eased_progress_at(t0 + Duration::from_millis(9_900));
    assert!(progress < 1.0, "expected no early snap, got {progress}");
    assert!((progress - 0.99).abs() < 1e-3, "got {progress}");
  }

  /// A slide completes once it is within one pixel of the target: at 99.9%
  /// progress over a 100px slide the residual is 0.1px, so it snaps to
  /// 1.0.
  #[test]
  fn long_slide_completes_within_one_pixel() {
    let anim = WindowAnimationState::new_movement(
      Rect::from_xy(0, 0, 100, 100),
      Rect::from_xy(100, 0, 100, 100),
      10_000,
      linear(),
    );

    let t0 = Instant::now();
    assert_eq!(anim.eased_progress_at(t0), 0.0);

    let progress =
      anim.eased_progress_at(t0 + Duration::from_millis(9_990));
    assert_eq!(progress, 1.0);
  }

  /// An opacity-only animation has zero positional travel, so completion
  /// falls back to the eased fraction: it is incomplete just below 99%
  /// and complete at/above it.
  #[test]
  fn opacity_only_completes_on_eased_fraction() {
    let anim = WindowAnimationState::new_movement(
      Rect::from_xy(0, 0, 100, 100),
      Rect::from_xy(0, 0, 100, 100),
      100,
      linear(),
    );

    let t0 = Instant::now();
    assert_eq!(anim.eased_progress_at(t0), 0.0);

    let before = anim.eased_progress_at(t0 + Duration::from_millis(98));
    assert!((before - 0.98).abs() < 1e-3, "got {before}");

    let after = anim.eased_progress_at(t0 + Duration::from_millis(99));
    assert_eq!(after, 1.0);
  }

  /// `start_delay` holds the animation at progress 0.0 until the delay
  /// elapses (used by the window-open paint grace period).
  #[test]
  fn start_delay_holds_at_zero() {
    let anim = WindowAnimationState::new_movement(
      Rect::from_xy(0, 0, 100, 100),
      Rect::from_xy(1_000, 0, 100, 100),
      100,
      linear(),
    )
    .with_delay(Duration::from_millis(30));

    let t0 = Instant::now();
    // Anchors `start_time`; still within the delay window.
    assert_eq!(anim.eased_progress_at(t0), 0.0);
    assert_eq!(
      anim.eased_progress_at(t0 + Duration::from_millis(20)),
      0.0
    );

    // 30ms in, the duration window has just begun (progress ~0, not
    // snapped).
    let after_delay =
      anim.eased_progress_at(t0 + Duration::from_millis(30));
    assert!(after_delay < 0.01, "got {after_delay}");

    // 30ms delay + 50ms into the 100ms duration → ~50% progress.
    let mid = anim.eased_progress_at(t0 + Duration::from_millis(80));
    assert!((mid - 0.5).abs() < 1e-2, "got {mid}");
  }
}
