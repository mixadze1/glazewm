// Tiling shares use f32; desktop pixel coordinates fit its integer
// precision.
#![allow(clippy::cast_precision_loss)]

use anyhow::Context;
use wm_common::TilingDirection;
#[cfg(target_os = "windows")]
use wm_platform::NativeWindowWindowsExt;
use wm_platform::Rect;

use crate::{
  models::{Container, TilingContainer, TilingWindow, WindowContainer},
  traits::{
    CommonGetters, PositionGetters, TilingDirectionGetters,
    TilingSizeGetters, WindowGetters, MIN_TILING_SIZE,
  },
};

pub fn refresh_window_minimum(
  window: &WindowContainer,
) -> anyhow::Result<()> {
  let current = window.to_rect().unwrap_or_else(|_| {
    window
      .native_properties()
      .frame
      .apply_delta(&window.border_delta().inverse(), None)
  });
  #[cfg(target_os = "windows")]
  let minimum = {
    let border_delta = window.total_border_delta()?;
    if window.native_properties().is_resizable {
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
    .unwrap_or(current)
  };
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
  Ok(())
}

pub fn refresh_tiling_minimums(
  container: &Container,
) -> anyhow::Result<()> {
  for child in
    std::iter::once(container.clone()).chain(container.descendants())
  {
    if let Ok(window @ WindowContainer::TilingWindow(_)) =
      child.as_window_container()
    {
      refresh_window_minimum(&window)?;
    }
  }
  Ok(())
}

/// Preserve descendant proportions while accounting for native minimums
/// and gaps. A simple sum does not protect uneven nested splits.
pub fn minimum_length(
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
        let value = minimum_length(child, horizontal)?;
        minimum = minimum.max(if same_axis {
          (value + 1.) / child.tiling_size().max(MIN_TILING_SIZE)
        } else {
          value
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

pub fn available_tiling_length(
  container: &TilingContainer,
  horizontal: bool,
) -> anyhow::Result<i32> {
  let parent = container.parent().context("No parent.")?;
  let rect = parent.to_rect()?;
  let (gap_x, gap_y) = container.inner_gaps()?;
  let count = i32::try_from(container.tiling_siblings().count())?;
  Ok(if horizontal {
    rect.width() - gap_x * count
  } else {
    rect.height() - gap_y * count
  })
}

pub fn resize_with_minimums(
  container: &TilingContainer,
  target: f32,
) -> anyhow::Result<bool> {
  let parent = container
    .parent()
    .context("No parent.")?
    .as_direction_container()?;
  let horizontal =
    parent.tiling_direction() == TilingDirection::Horizontal;
  let available = available_tiling_length(container, horizontal)? as f32;
  let siblings = container.tiling_siblings().collect::<Vec<_>>();
  if siblings.is_empty() || available <= 0. || !target.is_finite() {
    return Ok(false);
  }
  let floor = |child: &TilingContainer| -> anyhow::Result<f32> {
    Ok(
      ((minimum_length(child, horizontal)? + 1.) / available)
        .max(MIN_TILING_SIZE)
        .min(child.tiling_size()),
    )
  };
  let minimum = floor(container)?;
  let capacities = siblings
    .iter()
    .map(|child| Ok(child.tiling_size() - floor(child)?))
    .collect::<anyhow::Result<Vec<_>>>()?;
  let capacity: f32 = capacities.iter().sum();
  let size = container.tiling_size();
  let change = (target - size).clamp(minimum - size, capacity);
  if change.abs() < f32::EPSILON {
    return Ok(false);
  }
  let sibling_total: f32 =
    siblings.iter().map(TilingSizeGetters::tiling_size).sum();
  for (child, capacity_for_child) in siblings.iter().zip(capacities) {
    let weight = if change > 0. {
      capacity_for_child / capacity
    } else {
      child.tiling_size() / sibling_total
    };
    child.set_tiling_size(child.tiling_size() - change * weight);
  }
  container.set_tiling_size(size + change);
  Ok(true)
}

/// Plan before touching the tree, so an impossible insertion leaves the
/// existing layout exactly as it was.
pub fn plan_tiling_insertion(
  window: &TilingWindow,
  parent: &Container,
) -> anyhow::Result<Option<Vec<(TilingContainer, f32)>>> {
  let horizontal = parent.as_direction_container()?.tiling_direction()
    == TilingDirection::Horizontal;
  let rect = parent.to_rect()?;
  let mut children = parent.tiling_children().collect::<Vec<_>>();
  let (gap_x, gap_y) = match children.first() {
    Some(child) => child.inner_gaps()?,
    None => (0, 0),
  };
  let available = (if horizontal {
    rect.width()
  } else {
    rect.height()
  }) as f32
    - (if horizontal { gap_x } else { gap_y }) as f32
      * children.len() as f32;
  let cross_length = (if horizontal {
    rect.height()
  } else {
    rect.width()
  }) as f32;
  if available <= 0. {
    return Ok(None);
  }
  let new_share = 1. / (children.len() + 1) as f32;
  let mut weights = children
    .iter()
    .map(|c| c.tiling_size() * (1. - new_share))
    .collect::<Vec<_>>();
  weights.push(new_share);
  children.push(window.clone().into());
  let mut minimums = Vec::new();
  for child in &children {
    if minimum_length(child, !horizontal)? > cross_length {
      return Ok(None);
    }
    minimums.push(
      ((minimum_length(child, horizontal)? + 1.) / available)
        .max(MIN_TILING_SIZE),
    );
  }
  Ok(
    allocate_shares(&weights, &minimums)
      .map(|sizes| children.into_iter().zip(sizes).collect()),
  )
}

fn allocate_shares(weights: &[f32], minimums: &[f32]) -> Option<Vec<f32>> {
  if minimums.iter().sum::<f32>() > 1. {
    return None;
  }
  let mut result = vec![0.; weights.len()];
  let mut open = (0..weights.len()).collect::<Vec<_>>();
  let mut remaining = 1.;
  while !open.is_empty() {
    let total: f32 = open.iter().map(|&i| weights[i]).sum();
    let share = |i: usize| {
      if total > 0. {
        remaining * weights[i] / total
      } else {
        remaining / open.len() as f32
      }
    };
    let constrained = open
      .iter()
      .copied()
      .filter(|&i| share(i) < minimums[i])
      .collect::<Vec<_>>();
    if constrained.is_empty() {
      for &i in &open {
        result[i] = share(i);
      }
      break;
    }
    for i in constrained {
      result[i] = minimums[i];
      remaining -= minimums[i];
      open.retain(|&j| j != i);
    }
  }
  Some(result)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::models::{Monitor, SplitContainer, Workspace};

  fn window(width: i32, height: i32) -> TilingWindow {
    let window = TilingWindow::mock().call();
    window.update_native_properties(|p| {
      p.minimum_tiling_size = Some((width, height));
    });
    window
  }

  fn workspace(
    horizontal: bool,
    children: Vec<TilingContainer>,
  ) -> (Monitor, Workspace) {
    let workspace = Workspace::mock()
      .tiling_direction(if horizontal {
        TilingDirection::Horizontal
      } else {
        TilingDirection::Vertical
      })
      .tiling_containers(children)
      .call();
    let monitor =
      Monitor::mock().workspaces(vec![workspace.clone()]).call();
    (monitor, workspace)
  }

  #[test]
  fn keyboard_growth_and_shrink_respect_native_minimums_on_both_axes() {
    for horizontal in [true, false] {
      let a = window(300, 200);
      let b = window(500, 250);
      let (_monitor, _) =
        workspace(horizontal, vec![a.clone().into(), b.clone().into()]);
      let container = a.clone().into();
      resize_with_minimums(&container, 100.).unwrap();
      let b_rect = b.to_rect().unwrap();
      assert!(if horizontal {
        b_rect.width() >= 500
      } else {
        b_rect.height() >= 250
      });
      assert!(!resize_with_minimums(&container, 100.).unwrap());
      resize_with_minimums(&container, -100.).unwrap();
      let a_rect = a.to_rect().unwrap();
      assert!(if horizontal {
        a_rect.width() >= 300
      } else {
        a_rect.height() >= 200
      });
      assert!(!resize_with_minimums(&container, -100.).unwrap());
      assert!((a.tiling_size() + b.tiling_size() - 1.).abs() < 0.00001);
    }
  }

  #[test]
  fn keyboard_resize_uses_ancestor_parent_axis_for_nested_splits() {
    let a = window(200, 100);
    let b = window(400, 100);
    let c = window(200, 100);
    let split = SplitContainer::mock()
      .tiling_direction(TilingDirection::Vertical)
      .tiling_containers(vec![b.clone().into(), c.clone().into()])
      .call();
    let (_monitor, _) =
      workspace(true, vec![a.clone().into(), split.clone().into()]);
    resize_with_minimums(&split.into(), -100.).unwrap();
    assert!(b.to_rect().unwrap().width() >= 400);
    assert!(c.to_rect().unwrap().width() >= 400);
  }

  #[test]
  fn insertion_reserves_a_large_new_windows_minimum() {
    let a = window(300, 100);
    let b = window(300, 100);
    let new = window(900, 100);
    let (_monitor, parent) =
      workspace(true, vec![a.clone().into(), b.clone().into()]);
    let plan = plan_tiling_insertion(&new, &parent.clone().into())
      .unwrap()
      .unwrap();
    super::super::attach_container(
      &new.clone().into(),
      &parent.into(),
      None,
    )
    .unwrap();
    for (child, size) in plan {
      child.set_tiling_size(size);
    }
    assert!(new.to_rect().unwrap().width() >= 900);
    assert!(a.to_rect().unwrap().width() >= 300);
    assert!(b.to_rect().unwrap().width() >= 300);
  }

  #[test]
  fn impossible_insertion_does_not_modify_existing_layout() {
    let a = window(800, 100);
    let b = window(800, 100);
    let new = window(400, 100);
    let (_monitor, parent) =
      workspace(true, vec![a.clone().into(), b.clone().into()]);
    let before = (a.to_rect().unwrap(), b.to_rect().unwrap());
    assert!(plan_tiling_insertion(&new, &parent.into())
      .unwrap()
      .is_none());
    assert_eq!(before, (a.to_rect().unwrap(), b.to_rect().unwrap()));
    assert!(new.is_detached());
  }

  #[test]
  fn insertion_checks_cross_axis_and_nested_descendants() {
    let a = window(100, 100);
    let (_monitor, parent) = workspace(true, vec![a.into()]);
    let too_tall = window(100, 2000);
    assert!(plan_tiling_insertion(&too_tall, &parent.into())
      .unwrap()
      .is_none());
  }

  #[test]
  fn allocator_redistributes_without_nan_when_every_share_hits_its_minimum(
  ) {
    let result = allocate_shares(&[0., 0., 0.], &[0.2, 0.3, 0.5]).unwrap();
    assert!(result.iter().all(|size| size.is_finite()));
    assert!((result.iter().sum::<f32>() - 1.).abs() < 0.00001);
    for (actual, min) in result.iter().zip([0.2, 0.3, 0.5]) {
      assert!(*actual >= min);
    }
    assert!(allocate_shares(&[0.5, 0.5], &[0.7, 0.4]).is_none());
  }
}
