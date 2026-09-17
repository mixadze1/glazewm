use windows::Win32::{
  Foundation::HWND,
  UI::WindowsAndMessaging::{
    BeginDeferWindowPos, DeferWindowPos, EndDeferWindowPos,
    SWP_NOACTIVATE, SWP_NOSENDCHANGING, SWP_NOZORDER,
  },
};

use crate::{NativeWindow, NativeWindowWindowsExt, Rect, WindowZOrder};

/// Commit a live layout together, without asynchronous per-window queues
/// or forcing non-client frame recalculation on every pixel of the
/// gesture.
pub fn set_window_positions(
  windows: &[(NativeWindow, Rect)],
) -> crate::Result<()> {
  if windows.is_empty() {
    return Ok(());
  }
  let flags = SWP_NOACTIVATE | SWP_NOZORDER | SWP_NOSENDCHANGING;
  let batch = || -> crate::Result<()> {
    unsafe {
      let mut deferred =
        BeginDeferWindowPos(i32::try_from(windows.len())?)?;
      for (window, rect) in windows {
        deferred = DeferWindowPos(
          deferred,
          window.hwnd(),
          HWND(0),
          rect.x(),
          rect.y(),
          rect.width(),
          rect.height(),
          flags,
        )?;
      }
      EndDeferWindowPos(deferred)?;
    }
    Ok(())
  };
  if batch().is_err() {
    // A window may disappear during the gesture. Still update its
    // neighbors.
    for (window, rect) in windows {
      if window.is_valid() {
        window.set_window_pos(&WindowZOrder::Normal, rect, flags)?;
      }
    }
  }
  Ok(())
}
