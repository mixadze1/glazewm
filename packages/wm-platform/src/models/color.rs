use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct Color {
  pub r: u8,
  pub g: u8,
  pub b: u8,
  pub a: u8,
}

impl Color {
  #[must_use]
  #[allow(clippy::missing_panics_doc)]
  pub fn to_bgr(&self) -> u32 {
    u32::from(self.r)
      | (u32::from(self.g) << 8)
      | (u32::from(self.b) << 16)
  }
}

impl FromStr for Color {
  type Err = crate::ParseError;

  fn from_str(unparsed: &str) -> Result<Self, crate::ParseError> {
    // Validate before slicing: malformed config must not panic.
    if !matches!(unparsed.len(), 7 | 9) || !unparsed.is_ascii() {
      return Err(crate::ParseError::Color(unparsed.to_string()));
    }
    let mut chars = unparsed.chars();

    if chars.next() != Some('#') {
      return Err(crate::ParseError::Color(unparsed.to_string()));
    }

    let parse_hex = |slice: &str| -> Result<u8, crate::ParseError> {
      u8::from_str_radix(slice, 16)
        .map_err(|_| crate::ParseError::Color(unparsed.to_string()))
    };

    let r = parse_hex(&unparsed[1..3])?;
    let g = parse_hex(&unparsed[3..5])?;
    let b = parse_hex(&unparsed[5..7])?;

    let a = match unparsed.len() {
      9 => parse_hex(&unparsed[7..9])?,
      7 => 255,
      _ => return Err(crate::ParseError::Color(unparsed.to_string())),
    };

    Ok(Self { r, g, b, a })
  }
}

/// Deserialize a `Color` from either a string or a struct.
impl<'de> Deserialize<'de> for Color {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ColorDe {
      Struct { r: u8, g: u8, b: u8, a: u8 },
      String(String),
    }

    match ColorDe::deserialize(deserializer)? {
      ColorDe::Struct { r, g, b, a } => Ok(Self { r, g, b, a }),
      ColorDe::String(str) => {
        Self::from_str(&str).map_err(serde::de::Error::custom)
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn invalid_colors_return_errors_without_panicking() {
    for value in
      ["", "#", "#12", "#1234567", "#zz0000", "#é12345", "1234567"]
    {
      assert!(Color::from_str(value).is_err(), "{value}");
    }
  }

  #[test]
  fn hex_color_preserves_channels_and_alpha() {
    let color = Color::from_str("#12345678").unwrap();
    assert_eq!(color.to_bgr(), 0x0056_3412);
    assert_eq!(color.a, 0x78);
    assert_eq!(Color::from_str("#123456").unwrap().a, 255);
  }
}
