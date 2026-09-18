//! A focus-only, click-through outline, owned by the event-loop thread.
use std::{cell::Cell, marker::PhantomData, rc::Rc, sync::OnceLock};

use windows::{
  core::w,
  Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{
      Dwm::{
        DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT, DWMWCP_ROUND,
        DWMWCP_ROUNDSMALL, DWM_WINDOW_CORNER_PREFERENCE,
      },
      Gdi::{BeginPaint, EndPaint, PAINTSTRUCT},
    },
    UI::{
      HiDpi::GetDpiForWindow,
      WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow,
        GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsIconic,
        IsWindowVisible, IsZoomed, KillTimer, RegisterClassW, SetTimer,
        SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA,
        GWL_STYLE, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
        WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSW, WS_CAPTION,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TRANSPARENT, WS_POPUP, WS_THICKFRAME,
      },
    },
  },
};

use crate::{Color, NativeWindow};

mod bitmap;

// Win32 timer cadence, independent of the user's animation durations.
const TRACK_INTERVAL_MS: u32 = 16;
const TRACK_TIMER: usize = 1;
static CLASS: OnceLock<u16> = OnceLock::new();

struct OutlineState {
  target: HWND,
  color: Color,
  width: i32,
  last_shape: Cell<Option<(i32, i32, i32)>>,
}

impl OutlineState {
  fn track(&self, hwnd: HWND) {
    // SAFETY: Win32 validates target handles, including destroyed windows.
    unsafe {
      let mut frame = RECT::default();
      let mut cloaked = 0u32;
      let visible = self.target == GetForegroundWindow()
        && IsWindowVisible(self.target).as_bool()
        && !IsIconic(self.target).as_bool()
        && DwmGetWindowAttribute(
          self.target,
          DWMWA_CLOAKED,
          std::ptr::from_mut(&mut cloaked).cast(),
          u32::try_from(std::mem::size_of_val(&cloaked)).unwrap(),
        )
        .is_ok()
        && cloaked == 0
        && DwmGetWindowAttribute(
          self.target,
          DWMWA_EXTENDED_FRAME_BOUNDS,
          std::ptr::from_mut(&mut frame).cast(),
          u32::try_from(std::mem::size_of::<RECT>()).unwrap(),
        )
        .is_ok();
      if !visible {
        let _ = ShowWindow(hwnd, SW_HIDE);
        return;
      }
      let mut previous = RECT::default();
      let radius = window_corner_radius(self.target);
      let width = frame.right - frame.left;
      let height = frame.bottom - frame.top;
      let shape = (width, height, radius);
      if IsWindowVisible(hwnd).as_bool()
        && GetWindowRect(hwnd, &raw mut previous).is_ok()
        && previous == frame
        && self.last_shape.get() == Some(shape)
      {
        return;
      }
      if width <= 0 || height <= 0 {
        let _ = ShowWindow(hwnd, SW_HIDE);
        return;
      }
      // Reuse the composited bitmap when only the window position changes.
      if self.last_shape.get() != Some(shape) {
        if bitmap::render(hwnd, &frame, self.width, radius, &self.color)
          .is_err()
        {
          let _ = ShowWindow(hwnd, SW_HIDE);
          return;
        }
        self.last_shape.set(Some(shape));
      }
      let _ = SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        frame.left,
        frame.top,
        width,
        height,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
      );
    }
  }
}

/// DWM exposes a rounding preference, rather than the rendered contour.
/// Match standard Windows 11 radii; older Windows versions fall back to
/// square corners when the attribute is unavailable.
fn window_corner_radius(target: HWND) -> i32 {
  let mut preference = DWMWCP_DEFAULT;
  unsafe {
    let supported = DwmGetWindowAttribute(
      target,
      DWMWA_WINDOW_CORNER_PREFERENCE,
      std::ptr::from_mut(&mut preference).cast(),
      u32::try_from(std::mem::size_of_val(&preference)).unwrap(),
    )
    .is_ok();
    let style = GetWindowLongPtrW(target, GWL_STYLE);
    let frame_mask = isize::try_from(WS_CAPTION.0 | WS_THICKFRAME.0)
      .expect("Window frame style flags fit in an isize");
    let has_frame = style & frame_mask != 0;
    corner_radius(
      supported.then_some(preference),
      IsZoomed(target).as_bool(),
      has_frame,
      GetDpiForWindow(target).max(96),
    )
  }
}

fn corner_radius(
  preference: Option<DWM_WINDOW_CORNER_PREFERENCE>,
  maximized: bool,
  has_frame: bool,
  dpi: u32,
) -> i32 {
  if maximized {
    return 0;
  }
  let logical_radius = match preference {
    Some(DWMWCP_ROUND) => 8,
    Some(DWMWCP_ROUNDSMALL) => 4,
    Some(DWMWCP_DEFAULT) if has_frame => 8,
    _ => 0,
  };
  i32::try_from((logical_radius * u64::from(dpi) + 48) / 96).unwrap_or(0)
}

unsafe extern "system" fn window_proc(
  hwnd: HWND,
  message: u32,
  wparam: WPARAM,
  lparam: LPARAM,
) -> LRESULT {
  // SAFETY: The boxed state remains at a stable address until after
  // DestroyWindow; all access occurs on the creating event-loop thread.
  let state = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) }
    as *const OutlineState;
  if !state.is_null() {
    let state = unsafe { &*state };
    match message {
      WM_TIMER => {
        state.track(hwnd);
        return LRESULT(0);
      }
      WM_ERASEBKGND => return LRESULT(1),
      WM_PAINT => {
        // The per-pixel surface is supplied by UpdateLayeredWindow.
        unsafe {
          let mut paint = PAINTSTRUCT::default();
          let _ = BeginPaint(hwnd, &raw mut paint);
          let _ = EndPaint(hwnd, &raw const paint);
        }
        return LRESULT(0);
      }
      _ => {}
    }
  }
  unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

/// Keep inside `ThreadBound` when used off the event-loop thread.
pub struct FocusOutline {
  hwnd: HWND,
  state: Box<OutlineState>,
  _thread_bound: PhantomData<Rc<()>>,
}

impl FocusOutline {
  pub fn new(
    window: &NativeWindow,
    color: Color,
    width: u16,
  ) -> crate::Result<Self> {
    let atom = CLASS.get_or_init(|| unsafe {
      RegisterClassW(&WNDCLASSW {
        lpszClassName: w!("GlazeWM_FocusOutline"),
        lpfnWndProc: Some(window_proc),
        ..Default::default()
      })
    });
    if *atom == 0 {
      return Err(crate::Error::Platform(
        "Cannot register focus outline.".into(),
      ));
    }
    let mut outline = Self {
      hwnd: HWND(0),
      state: Box::new(OutlineState {
        target: HWND(window.id().0),
        color,
        width: i32::from(width),
        last_shape: Cell::new(None),
      }),
      _thread_bound: PhantomData,
    };
    // SAFETY: Registered class and boxed state outlive the window.
    unsafe {
      outline.hwnd = CreateWindowExW(
        WS_EX_LAYERED
          | WS_EX_TRANSPARENT
          | WS_EX_NOACTIVATE
          | WS_EX_TOOLWINDOW,
        w!("GlazeWM_FocusOutline"),
        w!(""),
        WS_POPUP,
        0,
        0,
        0,
        0,
        None,
        None,
        None,
        None,
      );
      if outline.hwnd.0 == 0 {
        return Err(crate::Error::Platform(
          "Cannot create focus outline.".into(),
        ));
      }
      SetWindowLongPtrW(
        outline.hwnd,
        GWLP_USERDATA,
        std::ptr::from_ref(outline.state.as_ref()) as isize,
      );
      if SetTimer(outline.hwnd, TRACK_TIMER, TRACK_INTERVAL_MS, None) == 0
      {
        return Err(crate::Error::Platform(
          "Cannot start focus outline timer.".into(),
        ));
      }
    }
    outline.state.track(outline.hwnd);
    Ok(outline)
  }

  pub fn update(
    &mut self,
    window: &NativeWindow,
    color: Color,
    width: u16,
  ) -> crate::Result<()> {
    self.state.target = HWND(window.id().0);
    self.state.color = color;
    self.state.width = i32::from(width);
    self.state.last_shape.set(None);
    unsafe {
      // Force geometry/region refresh even when only width changed.
      let _ = ShowWindow(self.hwnd, SW_HIDE);
    }
    self.state.track(self.hwnd);
    Ok(())
  }
}

impl Drop for FocusOutline {
  fn drop(&mut self) {
    if self.hwnd.0 == 0 {
      return;
    }
    // SAFETY: ThreadBound guarantees destruction on the creating thread.
    unsafe {
      let _ = KillTimer(self.hwnd, TRACK_TIMER);
      let _ = DestroyWindow(self.hwnd);
    }
  }
}

#[cfg(test)]
mod tests {
  use windows::Win32::Graphics::Dwm::DWMWCP_DONOTROUND;

  use super::*;

  #[test]
  fn outline_radius_matches_preference_and_dpi() {
    assert_eq!(corner_radius(Some(DWMWCP_ROUND), false, false, 96), 8);
    assert_eq!(corner_radius(Some(DWMWCP_ROUND), false, false, 144), 12);
    assert_eq!(
      corner_radius(Some(DWMWCP_ROUNDSMALL), false, false, 192),
      8
    );
    assert_eq!(corner_radius(Some(DWMWCP_DEFAULT), false, true, 96), 8);
    assert_eq!(corner_radius(Some(DWMWCP_DEFAULT), false, false, 96), 0);
    assert_eq!(corner_radius(Some(DWMWCP_DONOTROUND), false, true, 96), 0);
    assert_eq!(corner_radius(Some(DWMWCP_ROUND), true, true, 192), 0);
    assert_eq!(corner_radius(None, false, true, 96), 0);
  }

  #[test]
  fn outline_layered_surface_uploads_and_resizes() {
    // Exercise the real Windows compositing API without showing a window
    // or taking focus from the user's desktop.
    unsafe {
      let class = w!("GlazeWM_OutlineRenderTest");
      RegisterClassW(&WNDCLASSW {
        lpszClassName: class,
        lpfnWndProc: Some(window_proc),
        ..Default::default()
      });
      let hwnd = CreateWindowExW(
        WS_EX_LAYERED
          | WS_EX_TRANSPARENT
          | WS_EX_NOACTIVATE
          | WS_EX_TOOLWINDOW,
        class,
        w!(""),
        WS_POPUP,
        0,
        0,
        100,
        80,
        None,
        None,
        None,
        None,
      );
      assert_ne!(hwnd.0, 0);
      let color = Color {
        r: 100,
        g: 160,
        b: 255,
        a: 180,
      };
      let first = bitmap::render(
        hwnd,
        &RECT {
          left: 0,
          top: 0,
          right: 100,
          bottom: 80,
        },
        3,
        8,
        &color,
      );
      let second = bitmap::render(
        hwnd,
        &RECT {
          left: 10,
          top: 20,
          right: 210,
          bottom: 120,
        },
        4,
        12,
        &color,
      );
      let _ = DestroyWindow(hwnd);
      first.unwrap();
      second.unwrap();
    }
  }
}
