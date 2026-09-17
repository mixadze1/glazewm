use serde::{Deserialize, Serialize};
use wm_platform::Rect;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActiveDrag {
  /// Whether the drag is a move or resize.
  pub operation: Option<ActiveDragOperation>,

  /// Whether the drag is from a floating window.
  ///
  /// If `true`, it means we shouldn't drop the window as a tiling window
  /// on drag end.
  pub is_from_floating: bool,

  /// Initial position when the drag started.
  ///
  /// Used to calculate movement distance.
  pub initial_position: Rect,

  /// Edges selected at the beginning of a resize, before layout
  /// corrections.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub resize_edges: Option<ResizeEdges>,

  #[serde(skip)]
  pub initial_cursor_position: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
// Four independent physical edges; corners combine one edge on each axis.
#[allow(clippy::struct_excessive_bools)]
pub struct ResizeEdges {
  pub left: bool,
  pub top: bool,
  pub right: bool,
  pub bottom: bool,
}

#[derive(Debug, Copy, Clone, Deserialize, PartialEq, Serialize)]
pub enum ActiveDragOperation {
  Move,
  Resize,
}
