//! Antialiased, premultiplied BGRA surface for the click-through outline.
use windows::Win32::{
  Foundation::{COLORREF, HWND, POINT, RECT, SIZE},
  Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject,
    SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HDC, HGDIOBJ,
  },
  UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA},
};

use crate::Color;

pub(super) fn render(
  hwnd: HWND,
  frame: &RECT,
  thickness: i32,
  radius: i32,
  color: &Color,
) -> crate::Result<()> {
  let width = frame.right - frame.left;
  let height = frame.bottom - frame.top;
  let pixels = rasterize(width, height, thickness, radius, color)?;
  let info = BITMAPINFO {
    bmiHeader: BITMAPINFOHEADER {
      biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>())
        .unwrap(),
      biWidth: width,
      biHeight: -height, // Top-down pixels, matching screen coordinates.
      biPlanes: 1,
      biBitCount: 32,
      biCompression: BI_RGB.0,
      ..Default::default()
    },
    ..Default::default()
  };
  // SAFETY: The bitmap owns enough memory for width*height BGRA pixels.
  // The old DC selection is restored before the bitmap and DC are freed.
  unsafe {
    let dc = CreateCompatibleDC(HDC::default());
    if dc.0 == 0 {
      return Err(windows::core::Error::from_win32().into());
    }
    let mut bits = std::ptr::null_mut();
    let bitmap = match CreateDIBSection(
      dc,
      &raw const info,
      DIB_RGB_COLORS,
      &raw mut bits,
      None,
      0,
    ) {
      Ok(bitmap) => bitmap,
      Err(error) => {
        let _ = DeleteDC(dc);
        return Err(error.into());
      }
    };
    if bits.is_null() {
      let _ = DeleteObject(HGDIOBJ(bitmap.0));
      let _ = DeleteDC(dc);
      return Err(crate::Error::Platform(
        "Missing outline bitmap pixels".into(),
      ));
    }
    std::ptr::copy_nonoverlapping(
      pixels.as_ptr(),
      bits.cast::<u32>(),
      pixels.len(),
    );
    let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
    let result = UpdateLayeredWindow(
      hwnd,
      HDC::default(),
      Some(&POINT {
        x: frame.left,
        y: frame.top,
      }),
      Some(&SIZE {
        cx: width,
        cy: height,
      }),
      dc,
      Some(&POINT::default()),
      COLORREF(0),
      Some(&BLENDFUNCTION {
        BlendOp: u8::try_from(AC_SRC_OVER).unwrap(),
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: u8::try_from(AC_SRC_ALPHA).unwrap(),
      }),
      ULW_ALPHA,
    );
    SelectObject(dc, previous);
    let _ = DeleteObject(HGDIOBJ(bitmap.0));
    let _ = DeleteDC(dc);
    result?;
  }
  Ok(())
}

fn rasterize(
  width: i32,
  height: i32,
  thickness: i32,
  radius: i32,
  color: &Color,
) -> crate::Result<Vec<u32>> {
  if width <= 0 || height <= 0 {
    return Err(crate::Error::Platform(
      "Invalid outline dimensions".into(),
    ));
  }
  let columns = usize::try_from(width)?;
  let rows = usize::try_from(height)?;
  let count = columns.checked_mul(rows).ok_or_else(|| {
    crate::Error::Platform("Outline dimensions overflow".into())
  })?;
  let mut pixels = Vec::new();
  pixels.try_reserve_exact(count).map_err(|error| {
    crate::Error::Platform(format!("Cannot allocate outline: {error}"))
  })?;
  pixels.resize(count, 0);
  let radius = radius.max(0).min(width / 2).min(height / 2);
  let thickness = thickness.max(1).min((width.min(height) + 1) / 2);
  let band = usize::try_from(radius.max(thickness) + 1)?;
  for y in 0..rows {
    // Skip the transparent interior. Work scales with the perimeter,
    // rather than shading every pixel of a large window.
    let edge_row = y < band || y >= rows.saturating_sub(band);
    let spans = if edge_row || band * 2 >= columns {
      [0..columns, 0..0]
    } else {
      [0..band, columns - band..columns]
    };
    for x in spans.into_iter().flatten() {
      let coverage = ring_coverage(
        i32::try_from(x)?,
        i32::try_from(y)?,
        width,
        height,
        thickness,
        radius,
      );
      pixels[y * columns + x] = premultiplied_pixel(color, coverage);
    }
  }
  Ok(pixels)
}

fn ring_coverage(
  x: i32,
  y: i32,
  width: i32,
  height: i32,
  inset: i32,
  radius: i32,
) -> f64 {
  let x = f64::from(x) + 0.5;
  let y = f64::from(y) + 0.5;
  let outer =
    coverage(x, y, f64::from(width), f64::from(height), f64::from(radius));
  let inner = if width > inset * 2 && height > inset * 2 {
    coverage(
      x - f64::from(inset),
      y - f64::from(inset),
      f64::from(width - inset * 2),
      f64::from(height - inset * 2),
      f64::from((radius - inset).max(0)),
    )
  } else {
    0.0
  };
  (outer - inner).clamp(0.0, 1.0)
}

/// Signed distance to a rounded rectangle gives a one-pixel coverage
/// ramp along both curves. Straight pixel-aligned edges remain crisp.
fn coverage(x: f64, y: f64, width: f64, height: f64, radius: f64) -> f64 {
  let qx = (x - width / 2.0).abs() - (width / 2.0 - radius);
  let qy = (y - height / 2.0).abs() - (height / 2.0 - radius);
  let distance =
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
  (0.5 - distance).clamp(0.0, 1.0)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn premultiplied_pixel(color: &Color, coverage: f64) -> u32 {
  let alpha = (f64::from(color.a) * coverage).round() as u32;
  let channel = |value: u8| (u32::from(value) * alpha + 127) / 255;
  (alpha << 24)
    | (channel(color.r) << 16)
    | (channel(color.g) << 8)
    | channel(color.b)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn outline_pixels_have_smooth_symmetric_corners_and_empty_center() {
    let color = Color {
      r: 100,
      g: 160,
      b: 255,
      a: 255,
    };
    for radius in [4, 8, 12, 16] {
      let pixels = rasterize(100, 80, 3, radius, &color).unwrap();
      assert_eq!(pixels[40 * 100 + 50], 0);
      assert_eq!(pixels[50] >> 24, 255);
      assert_eq!(pixels[2 * 100 + 50] >> 24, 255);
      assert_eq!(pixels[3 * 100 + 50], 0);
      assert!(pixels
        .iter()
        .any(|pixel| (1..255).contains(&(pixel >> 24))));
      for y in 0..80 {
        for x in 0..100 {
          assert_eq!(pixels[y * 100 + x], pixels[y * 100 + 99 - x]);
          assert_eq!(pixels[y * 100 + x], pixels[(79 - y) * 100 + x]);
        }
      }
    }
  }

  #[test]
  fn outline_alpha_is_premultiplied_including_configured_opacity() {
    let color = Color {
      r: 255,
      g: 128,
      b: 64,
      a: 128,
    };
    assert_eq!(premultiplied_pixel(&color, 0.5), 0x4040_2010);
    assert_eq!(premultiplied_pixel(&color, 0.0), 0);
    let pixels = rasterize(100, 80, 3, 8, &color).unwrap();
    for pixel in pixels {
      let alpha = pixel >> 24;
      assert!(alpha <= 128);
      for shift in [0, 8, 16] {
        assert!((pixel >> shift) & 255 <= alpha);
      }
    }
  }

  #[test]
  fn outline_respects_configured_thickness() {
    let color = Color {
      r: 255,
      g: 255,
      b: 255,
      a: 255,
    };
    for thickness in [2, 3, 6, 12] {
      let pixels = rasterize(100, 80, thickness, 8, &color).unwrap();
      for y in 0..40 {
        let expected = if y < usize::try_from(thickness).unwrap() {
          255
        } else {
          0
        };
        assert_eq!(pixels[y * 100 + 50] >> 24, expected);
      }
      for x in 0..50 {
        let expected = if x < usize::try_from(thickness).unwrap() {
          255
        } else {
          0
        };
        assert_eq!(pixels[40 * 100 + x] >> 24, expected);
      }
    }
  }

  #[test]
  fn outline_square_and_tiny_windows_remain_valid() {
    let color = Color {
      r: 255,
      g: 255,
      b: 255,
      a: 255,
    };
    let pixels = rasterize(100, 80, 3, 0, &color).unwrap();
    assert_eq!(pixels[0], u32::MAX);
    assert_eq!(pixels[7999], u32::MAX);
    assert!(pixels.iter().all(|pixel| *pixel == 0 || *pixel == u32::MAX));
    assert_eq!(rasterize(1, 1, 10, 8, &color).unwrap(), vec![u32::MAX]);
    assert!(rasterize(0, 80, 3, 8, &color).is_err());
  }
}
