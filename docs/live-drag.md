# Live tile reordering

On the `feature/smooth-scroll` branch, dragging a tiled window by its title
bar updates the layout before the mouse button is released. The dragged
window keeps its native size and follows the pointer; its reserved tile
moves through the layout while neighboring windows animate to their new
positions. Releasing the mouse animates the dragged window into its slot.

```yaml
window_behavior:
  live_drag_reordering: true
```

This option defaults to `true`. Set it to `false` and reload the configuration
to restore placement only on mouse release. Movement and resize animations
use the existing animation settings; enable those animations for smooth
transitions. Disabling animations still allows live reordering.

The pointer must leave the current slot by 16 pixels and enter another tile
by 16 pixels before its placement is reconsidered. Hit testing uses final
layout rectangles rather than moving animation frames, avoiding repeated
swaps while the pointer is stationary. Depending on the pointer's position,
placement can reorder siblings or create a horizontal/vertical split.
Moving onto an empty monitor's visible workspace is supported.

Floating windows keep their usual free movement. Native edge resizing and
dragging out of fullscreen keep their existing behavior.

Regression tests cover changes before release, stable repeated pointer
positions, boundary jitter, nested splits, movement between monitors, and
configuration defaults. They exercise the layout tree without controlling
desktop windows; perceived smoothness still needs an interactive check.
