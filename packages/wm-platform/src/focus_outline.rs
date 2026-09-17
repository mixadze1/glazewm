//! A focus-only, click-through outline, owned by the event-loop thread.
use std::{marker::PhantomData, rc::Rc, sync::OnceLock};

use windows::{
  core::w,
  Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{
      Dwm::{
        DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
      },
      Gdi::{
        BeginPaint, CombineRgn, CreateRectRgn, CreateSolidBrush,
        DeleteObject, EndPaint, FillRect, InvalidateRect, SetWindowRgn,
        HGDIOBJ, PAINTSTRUCT, RGN_DIFF,
      },
    },
    UI::WindowsAndMessaging::{
      CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow,
      GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindowVisible,
      KillTimer, RegisterClassW, SetLayeredWindowAttributes, SetTimer,
      SetWindowLongPtrW, SetWindowPos, ShowWindow, GWLP_USERDATA,
      HWND_TOPMOST, LWA_ALPHA, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE,
      WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
      WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
    },
  },
};

use crate::{Color, NativeWindow};

// Win32 timer cadence, independent of the user's animation durations.
const TRACK_INTERVAL_MS: u32 = 16;
const TRACK_TIMER: usize = 1;
static CLASS: OnceLock<u16> = OnceLock::new();

struct OutlineState {
  target: HWND,
  color: Color,
  width: i32,
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
      if IsWindowVisible(hwnd).as_bool()
        && GetWindowRect(hwnd, &raw mut previous).is_ok()
        && previous == frame
      {
        return;
      }
      let width = frame.right - frame.left;
      let height = frame.bottom - frame.top;
      let inset = self.width.min(width / 2).min(height / 2).max(1);
      let outer = CreateRectRgn(0, 0, width, height);
      let inner =
        CreateRectRgn(inset, inset, width - inset, height - inset);
      if outer.0 == 0 || inner.0 == 0 {
        let _ = DeleteObject(HGDIOBJ(outer.0));
        let _ = DeleteObject(HGDIOBJ(inner.0));
        let _ = ShowWindow(hwnd, SW_HIDE);
        return;
      }
      let combined = CombineRgn(outer, outer, inner, RGN_DIFF);
      let _ = DeleteObject(HGDIOBJ(inner.0));
      // Windows takes ownership only when SetWindowRgn succeeds.
      if combined.0 == 0 || SetWindowRgn(hwnd, outer, true) == 0 {
        let _ = DeleteObject(HGDIOBJ(outer.0));
        let _ = ShowWindow(hwnd, SW_HIDE);
        return;
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
        // SAFETY: The paint DC and brush are paired with their cleanup.
        unsafe {
          let mut paint = PAINTSTRUCT::default();
          let dc = BeginPaint(hwnd, &raw mut paint);
          let brush = CreateSolidBrush(COLORREF(state.color.to_bgr()));
          if brush.0 != 0 {
            FillRect(dc, &raw const paint.rcPaint, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
          }
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
      SetLayeredWindowAttributes(
        outline.hwnd,
        COLORREF(0),
        outline.state.color.a,
        LWA_ALPHA,
      )?;
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
    unsafe {
      // Force geometry/region refresh even when only width changed.
      let _ = ShowWindow(self.hwnd, SW_HIDE);
      SetLayeredWindowAttributes(
        self.hwnd,
        COLORREF(0),
        self.state.color.a,
        LWA_ALPHA,
      )?;
      let _ = InvalidateRect(self.hwnd, None, false);
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
