#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
  use wm_platform::{NativeWindow, NativeWindowWindowsExt};

  for argument in std::env::args().skip(1) {
    let handle = argument.parse::<isize>()?;
    let window = NativeWindow::from_handle(handle);
    println!(
      "{}",
      serde_json::json!({
        "handle": handle,
        "frame": window.frame().ok(),
        "outerFrame": window.frame_with_shadows().ok(),
        "cloaked": window.is_cloaked().ok(),
        "minimized": window.is_minimized().ok(),
        "maximized": window.is_maximized().ok(),
      })
    );
  }
  Ok(())
}

#[cfg(not(target_os = "windows"))]
fn main() {}
