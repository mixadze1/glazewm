use wm_common::EasingFunction;

use super::engine::apply_easing;

/// A camera moving across workspace panels. Retargeting preserves the
/// last submitted position AND velocity, independently of window handles.
pub(super) struct WorkspaceMotion {
  pub position: f32,
  pub velocity: f32,
  start: f32,
  target: f32,
  initial_velocity: f32,
  retargeted: bool,
}

impl WorkspaceMotion {
  pub fn new(start: f32, target: f32) -> Self {
    Self {
      position: start,
      velocity: 0.0,
      start,
      target,
      initial_velocity: 0.0,
      retargeted: false,
    }
  }

  pub fn retarget(&mut self, target: f32) {
    self.start = self.position;
    self.initial_velocity = self.velocity;
    self.target = target;
    self.retargeted = true;
  }

  pub fn sample(
    &mut self,
    progress: f32,
    seconds: f32,
    easing: &EasingFunction,
  ) {
    if progress >= 1.0 || seconds <= 0.0 {
      self.position = self.target;
      self.velocity = 0.0;
      return;
    }
    let t = progress.clamp(0.0, 1.0);
    let distance = self.target - self.start;
    if self.retargeted {
      // Cubic Hermite: preserve incoming velocity and arrive at rest.
      let tangent = self.initial_velocity * seconds;
      let blend = t * t * (3.0 - 2.0 * t);
      let momentum = t * (1.0 - t) * (1.0 - t);
      self.position = self.start + distance * blend + tangent * momentum;
      self.velocity = (distance * 6.0 * t * (1.0 - t)
        + tangent * (1.0 - 4.0 * t + 3.0 * t * t))
        / seconds;
    } else {
      self.position = self.start + distance * apply_easing(t, easing);
      // Numerical derivative of the configured curve, in panels/second.
      const DERIVATIVE_STEP: f32 = 0.001;
      let low = (t - DERIVATIVE_STEP).max(0.0);
      let high = (t + DERIVATIVE_STEP).min(1.0);
      self.velocity = distance
        * (apply_easing(high, easing) - apply_easing(low, easing))
        / ((high - low) * seconds);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn retargets_preserve_position_and_velocity_in_both_directions() {
    let easing = EasingFunction::default();
    let mut motion = WorkspaceMotion::new(0.0, 1.0);
    for target in [2.0, 0.0, 3.0, 1.0, 2.0, 0.0] {
      motion.sample(0.3, 0.45, &easing);
      let before = (motion.position, motion.velocity);
      motion.retarget(target);
      motion.sample(0.0, 0.45, &easing);
      assert!((motion.position - before.0).abs() < 0.00001);
      assert!((motion.velocity - before.1).abs() < 0.00001);
      // Every panel retains its screen offset, including older outgoing
      // panels that no longer belong to either end of the newest route.
      for panel in 0..4 {
        assert!(
          ((panel as f32 - motion.position) - (panel as f32 - before.0))
            .abs()
            < 0.00001
        );
      }
    }
    motion.sample(1.0, 0.45, &easing);
    assert_eq!(motion.position, 0.0);
    assert_eq!(motion.velocity, 0.0);
  }

  #[test]
  fn retarget_continues_moving_on_the_next_frame_and_finishes_at_rest() {
    let easing = EasingFunction::default();
    let mut motion = WorkspaceMotion::new(0.0, 1.0);
    motion.sample(0.4, 0.45, &easing);
    let position = motion.position;
    let velocity = motion.velocity;
    motion.retarget(2.0);
    let dt = 0.0001;
    motion.sample(dt / 0.45, 0.45, &easing);
    assert!(motion.position > position);
    assert!(((motion.position - position) / dt - velocity).abs() < 0.01);
    motion.sample(1.0, 0.45, &easing);
    assert_eq!(motion.position, 2.0);
    assert_eq!(motion.velocity, 0.0);
  }

  #[test]
  fn retarget_before_first_frame_and_zero_duration_reach_latest_target() {
    let mut motion = WorkspaceMotion::new(0.0, 1.0);
    motion.retarget(2.0);
    motion.retarget(3.0);
    motion.sample(0.0, 0.45, &EasingFunction::default());
    assert_eq!(motion.position, 0.0);
    motion.sample(0.0, 0.0, &EasingFunction::default());
    assert_eq!(motion.position, 3.0);
    assert_eq!(motion.velocity, 0.0);
  }
}
