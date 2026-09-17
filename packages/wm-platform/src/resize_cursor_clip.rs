use windows::Win32::{
  Foundation::RECT,
  UI::WindowsAndMessaging::{ClipCursor, GetClipCursor},
};

/// Keeps the native sizing loop inside the legal tile range. Releases
/// cursor confinement when the gesture ends, including error paths.
pub struct ResizeCursorClip {
  previous: RECT,
  bounds: RECT,
  horizontal: bool,
  vertical: bool,
}

impl ResizeCursorClip {
  pub fn new() -> crate::Result<Self> {
    let mut previous = RECT::default();
    unsafe {
      GetClipCursor(&raw mut previous)?;
    }
    Ok(Self {
      previous,
      bounds: previous,
      horizontal: false,
      vertical: false,
    })
  }

  /// Each axis is frozen once selected, so rounding during layout updates
  /// cannot move the limit back and forth beneath a stationary cursor.
  pub fn constrain_axis(
    &mut self,
    horizontal: bool,
    min: i32,
    max: i32,
  ) -> crate::Result<()> {
    if (horizontal && self.horizontal) || (!horizontal && self.vertical) {
      return Ok(());
    }
    let mut bounds = self.bounds;
    if horizontal {
      bounds.left =
        min.max(self.previous.left).min(self.previous.right - 1);
      bounds.right = max
        .saturating_add(1)
        .min(self.previous.right)
        .max(bounds.left + 1);
    } else {
      bounds.top =
        min.max(self.previous.top).min(self.previous.bottom - 1);
      bounds.bottom = max
        .saturating_add(1)
        .min(self.previous.bottom)
        .max(bounds.top + 1);
    }
    unsafe {
      ClipCursor(Some(&raw const bounds))?;
    }
    self.bounds = bounds;
    if horizontal {
      self.horizontal = true;
    } else {
      self.vertical = true;
    }
    Ok(())
  }
}

impl Drop for ResizeCursorClip {
  fn drop(&mut self) {
    unsafe {
      // The captured rect can be Windows' temporary sizing restriction.
      // Restoring it after MOVESIZEEND would trap the cursor on that
      // monitor.
      let _ = ClipCursor(None);
    }
  }
}
