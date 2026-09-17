use crate::{
  models::TilingContainer,
  traits::{CommonGetters, TilingSizeGetters, MIN_TILING_SIZE},
};

pub fn resize_tiling_container(
  container_to_resize: &TilingContainer,
  target_size: f32,
) {
  if !target_size.is_finite() {
    return;
  }
  let tiling_siblings =
    container_to_resize.tiling_siblings().collect::<Vec<_>>();

  // Ignore cases where the container is the only child.
  if tiling_siblings.is_empty() {
    container_to_resize.set_tiling_size(1.);
    return;
  }

  // Prevent the container from being smaller than the minimum size, and
  // larger than the space available from sibling containers.
  #[allow(clippy::cast_precision_loss)]
  let clamped_target_size = target_size.clamp(
    MIN_TILING_SIZE,
    1. - (tiling_siblings.len() as f32 * MIN_TILING_SIZE),
  );

  let size_delta = clamped_target_size - container_to_resize.tiling_size();
  container_to_resize.set_tiling_size(clamped_target_size);

  // Get available tiling size amongst siblings.
  let available_size =
    tiling_siblings.iter().fold(0.0, |sum, container| {
      sum + container.tiling_size() - MIN_TILING_SIZE
    });

  // Distribute the available tiling size amongst its siblings.
  for sibling in &tiling_siblings {
    // Get percentage of resize that affects this container. Siblings are
    // resized in proportion to their current size (i.e. larger containers
    // are shrunk more).
    // When every sibling is at its minimum, shrinking the target must
    // still work rather than writing NaN proportions into the tree.
    #[allow(clippy::cast_precision_loss)]
    let resize_factor = if available_size > f32::EPSILON {
      (sibling.tiling_size() - MIN_TILING_SIZE) / available_size
    } else {
      1. / tiling_siblings.len() as f32
    };

    let size_delta = resize_factor * size_delta;

    sibling.set_tiling_size(sibling.tiling_size() - size_delta);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::{TilingWindow, Workspace};

  #[test]
  fn shrinking_with_minimum_siblings_keeps_finite_normalized_shares() {
    let a = TilingWindow::mock().call();
    let b = TilingWindow::mock().call();
    let _workspace = Workspace::mock()
      .tiling_containers(vec![a.clone().into(), b.clone().into()])
      .call();
    a.set_tiling_size(1. - MIN_TILING_SIZE);
    b.set_tiling_size(MIN_TILING_SIZE);
    resize_tiling_container(&a.clone().into(), 0.5);
    assert!((a.tiling_size() - 0.5).abs() < f32::EPSILON);
    assert!((b.tiling_size() - 0.5).abs() < f32::EPSILON);
    resize_tiling_container(&a.clone().into(), f32::NAN);
    assert!(a.tiling_size().is_finite());
  }
}
