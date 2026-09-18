use windows::Win32::{
  Foundation::{BOOL, HWND, LPARAM},
  Graphics::Gdi::{MonitorFromWindow, HMONITOR, MONITOR_DEFAULTTONEAREST},
  UI::WindowsAndMessaging::EnumWindows,
};

use super::native_window::NativeWindow;
use crate::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Edge {
  Top,
  Bottom,
  Left,
  Right,
}

struct Panel {
  edge: Edge,
  depth: i32,
  visible: bool,
}

/// Augment Windows' work area with edge bars that do not register an
/// appbar. Only Zebar and the Windows taskbars are eligible.
pub(super) fn working_area(
  monitor: HMONITOR,
  bounds: &Rect,
  system_work: &Rect,
  dpi: u32,
) -> crate::Result<Rect> {
  let mut handles = Vec::<isize>::new();
  unsafe {
    EnumWindows(
      Some(collect_window),
      LPARAM(std::ptr::from_mut(&mut handles) as isize),
    )
  }?;

  // Allow a small margin between the bar and the edge (in logical px).
  let edge_margin = i32::try_from(dpi).unwrap_or(96) * 64 / 96;
  let mut panels = Vec::new();
  for handle in handles {
    let window = NativeWindow::new(handle);
    let Ok(frame) = window.frame_with_shadows() else {
      continue;
    };
    let Some(edge) = panel_edge(bounds, &frame, edge_margin) else {
      continue;
    };
    // Hidden/auto-hidden taskbars may be just outside the display.
    if unsafe { MonitorFromWindow(HWND(handle), MONITOR_DEFAULTTONEAREST) }
      != monitor
    {
      continue;
    }
    let is_taskbar = window.class_name().is_ok_and(|name| {
      matches!(name.as_str(), "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
    });
    if !is_taskbar
      && !window
        .process_name()
        .is_ok_and(|name| name.eq_ignore_ascii_case("zebar"))
    {
      continue;
    }

    let depth = edge.depth(bounds, &frame);
    // Auto-hide leaves a one/two pixel activation strip. It is not a bar.
    let visible = depth > 2
      && bounds.intersection_area(&frame) > 0
      && window.is_visible().unwrap_or(false)
      && !window.is_minimized().unwrap_or(true);
    let depth = if !visible && is_taskbar {
      match edge {
        Edge::Top | Edge::Bottom => frame.height(),
        Edge::Left | Edge::Right => frame.width(),
      }
    } else {
      depth
    };
    panels.push(Panel {
      edge,
      depth,
      visible,
    });
  }

  Ok(resolve_working_area(bounds, system_work, &panels))
}

extern "system" fn collect_window(hwnd: HWND, data: LPARAM) -> BOOL {
  // SAFETY: EnumWindows calls this synchronously; the vector outlives it.
  unsafe { (*(data.0 as *mut Vec<isize>)).push(hwnd.0) };
  true.into()
}

/// Require an elongated window spanning at least half the monitor and
/// close to an edge. Zebar settings windows and small widgets don't count.
fn panel_edge(bounds: &Rect, frame: &Rect, margin: i32) -> Option<Edge> {
  if frame.width() <= 0 || frame.height() <= 0 {
    return None;
  }
  let horizontal_overlap =
    bounds.right.min(frame.right) - bounds.left.max(frame.left);
  let vertical_overlap =
    bounds.bottom.min(frame.bottom) - bounds.top.max(frame.top);
  if horizontal_overlap >= bounds.width() / 2
    && frame.height() <= bounds.height() / 4
    && frame.width() >= frame.height() * 4
  {
    let top_distance = (frame.top - bounds.top).abs();
    let bottom_distance = (frame.bottom - bounds.bottom).abs();
    if top_distance.min(bottom_distance) <= margin {
      return Some(if top_distance <= bottom_distance {
        Edge::Top
      } else {
        Edge::Bottom
      });
    }
  }
  if vertical_overlap >= bounds.height() / 2
    && frame.width() <= bounds.width() / 4
    && frame.height() >= frame.width() * 4
  {
    let left_distance = (frame.left - bounds.left).abs();
    let right_distance = (frame.right - bounds.right).abs();
    if left_distance.min(right_distance) <= margin {
      return Some(if left_distance <= right_distance {
        Edge::Left
      } else {
        Edge::Right
      });
    }
  }
  None
}

impl Edge {
  fn depth(self, bounds: &Rect, panel: &Rect) -> i32 {
    match self {
      Self::Top => panel.bottom - bounds.top,
      Self::Bottom => bounds.bottom - panel.top,
      Self::Left => panel.right - bounds.left,
      Self::Right => bounds.right - panel.left,
    }
  }

  fn inset(self, bounds: &Rect, work: &Rect) -> i32 {
    match self {
      Self::Top => work.top - bounds.top,
      Self::Bottom => bounds.bottom - work.bottom,
      Self::Left => work.left - bounds.left,
      Self::Right => bounds.right - work.right,
    }
  }

  fn set_inset(self, bounds: &Rect, work: &mut Rect, inset: i32) {
    match self {
      Self::Top => work.top = bounds.top + inset,
      Self::Bottom => work.bottom = bounds.bottom - inset,
      Self::Left => work.left = bounds.left + inset,
      Self::Right => work.right = bounds.right - inset,
    }
  }
}

fn resolve_working_area(
  bounds: &Rect,
  system_work: &Rect,
  panels: &[Panel],
) -> Rect {
  let mut work = system_work.clone();
  // Release only reservations covered by a known hidden panel. Preserve
  // unrelated appbars, then apply visible panels so enumeration order
  // cannot make a hidden panel erase a visible one on the same edge.
  for panel in panels.iter().filter(|panel| !panel.visible) {
    let reserved = panel.edge.inset(bounds, &work);
    if reserved > 0 && reserved <= panel.depth {
      panel.edge.set_inset(bounds, &mut work, 0);
    }
  }
  for panel in panels.iter().filter(|panel| panel.visible) {
    let inset = panel.edge.inset(bounds, &work).max(panel.depth);
    panel.edge.set_inset(bounds, &mut work, inset);
  }
  if work.width() <= 0 || work.height() <= 0 {
    system_work.clone()
  } else {
    work
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn zebar_appears_resizes_and_disappears() {
    let bounds = Rect::from_ltrb(0, 0, 1920, 1080);
    let system = Rect::from_ltrb(0, 0, 1920, 1032);
    for height in [32, 48] {
      let panel = Panel {
        edge: Edge::Top,
        depth: height,
        visible: true,
      };
      assert_eq!(
        resolve_working_area(&bounds, &system, &[panel]),
        Rect::from_ltrb(0, height, 1920, 1032),
      );
    }
    assert_eq!(resolve_working_area(&bounds, &system, &[]), system);
  }

  #[test]
  fn registered_bar_is_not_counted_twice() {
    let bounds = Rect::from_ltrb(0, 0, 1920, 1080);
    let system = Rect::from_ltrb(0, 40, 1920, 1032);
    let panels = [
      Panel {
        edge: Edge::Top,
        depth: 40,
        visible: true,
      },
      Panel {
        edge: Edge::Bottom,
        depth: 48,
        visible: true,
      },
    ];
    assert_eq!(resolve_working_area(&bounds, &system, &panels), system);
  }

  #[test]
  fn hidden_bars_release_their_reservations() {
    let bounds = Rect::from_ltrb(0, 0, 1920, 1080);
    let panels = [
      Panel {
        edge: Edge::Top,
        depth: 40,
        visible: false,
      },
      Panel {
        edge: Edge::Bottom,
        depth: 48,
        visible: false,
      },
    ];
    for bottom in [1032, 1078] {
      let system = Rect::from_ltrb(0, 40, 1920, bottom);
      assert_eq!(resolve_working_area(&bounds, &system, &panels), bounds);
    }
  }

  #[test]
  fn hidden_bar_does_not_remove_visible_bar_or_larger_reservation() {
    let bounds = Rect::from_ltrb(0, 0, 1920, 1080);
    let system = Rect::from_ltrb(0, 40, 1920, 1000);
    let panels = [
      Panel {
        edge: Edge::Top,
        depth: 40,
        visible: false,
      },
      Panel {
        edge: Edge::Top,
        depth: 32,
        visible: true,
      },
      Panel {
        edge: Edge::Bottom,
        depth: 48,
        visible: false,
      },
    ];
    assert_eq!(
      resolve_working_area(&bounds, &system, &panels),
      Rect::from_ltrb(0, 32, 1920, 1000),
    );
  }

  #[test]
  fn panel_detection_handles_negative_monitor_coordinates_and_margins() {
    let bounds = Rect::from_ltrb(-2560, -200, 0, 1240);
    assert_eq!(
      panel_edge(&bounds, &Rect::from_ltrb(-2550, -190, -10, -150), 64),
      Some(Edge::Top)
    );
    assert_eq!(
      panel_edge(&bounds, &Rect::from_ltrb(-2560, 1192, 0, 1240), 64),
      Some(Edge::Bottom)
    );
    assert_eq!(
      panel_edge(&bounds, &Rect::from_ltrb(-2560, -200, -2520, 1240), 64),
      Some(Edge::Left)
    );
    assert_eq!(
      panel_edge(&bounds, &Rect::from_ltrb(-40, -200, 0, 1240), 64),
      Some(Edge::Right)
    );
    // Other monitor, central overlay, settings window, small widget.
    for frame in [
      Rect::from_ltrb(0, 0, 1920, 40),
      Rect::from_ltrb(-2560, 400, 0, 440),
      Rect::from_ltrb(-2400, -200, -500, 800),
      Rect::from_ltrb(-2560, -200, -2460, -160),
    ] {
      assert_eq!(panel_edge(&bounds, &frame, 64), None);
    }
  }
}
