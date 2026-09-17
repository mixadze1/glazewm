pub mod engine;
pub mod manager;
pub mod state;
#[cfg(target_os = "windows")]
mod workspace_motion;

pub use manager::{AnimationManager, AnimationPositionResult};
