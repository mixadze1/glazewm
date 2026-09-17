# Windows animation preview

Branch: `feature/improves-animations`, based on upstream PR #1392 at
`d76641418f9642837b63817a8eeed7fbed4aadb5`.

Add this top-level section to `%USERPROFILE%\.glzr\glazewm\config.yaml`:

```yaml
animations:
  window_move:
    enabled: true
    duration_ms: 150
    easing: ease_out_cubic
    threshold_px: 2
  window_resize:
    enabled: true
    duration_ms: 150
    easing: ease_out_cubic
    threshold_px: 2
  window_open:
    enabled: true
    duration_ms: 150
    easing: ease_out_cubic
    style: slide_right
    opacity_from: 1.0
  workspace_switch:
    enabled: true
    duration_ms: 200
    easing: ease_out_cubic
    style: slide
    direction: horizontal
    opacity_outgoing: 1.0
    opacity_incoming: 1.0
    zoom_factor: 0.0
  window_close:
    enabled: false
```

Reload with `Alt+Shift+R`. Each animation has an independent `enabled` flag
and duration in milliseconds. Omitted animations default to disabled.
The bundled sample config explicitly enables animations.

Window open/close styles: `slide_right`, `slide_left`, `slide_top`,
`slide_bottom`, `none` (also `fade`), and `zoom`.
Workspace styles: `slide`, `fade`, `zoom`, and `iris`; slide direction is
`horizontal` or `vertical`. For fades, configure opacity as well as style.
Opacity and zoom values must be finite and in [0, 1].

Easing accepts `linear`, `ease_in`, `ease_out`, `ease_in_out`,
`ease_in_cubic`, `ease_out_cubic`, `ease_in_out_cubic`, `ease_out_spring`,
or `cubic_bezier(x1, y1, x2, y2)`. All control points must be finite;
x coordinates must be in [0, 1].

Compatibility aliases: `type` for `style`, and `direction` for the opening
window's `style`. The workspace's `direction` continues to select its axis.

Review fixes in this branch:

- Restore missing style aliases that were silently ignored upstream.
- Validate opacity, zoom and non-finite cubic-bezier values during config load.
- Keep old configs animation-free unless explicitly enabled.
- Preserve submillisecond timing precision and handle flat bezier derivatives.
- Add regression coverage for aliases, settings, defaults, invalid numbers,
  zero/submillisecond timing and difficult bezier curves.

This is an experimental build, not an upstream release. Automated tests
cannot establish compatibility with every game, GPU, elevated application,
or monitor/DPI combination. Unsigned builds do not use UIAccess and cannot
manage elevated windows with the same privileges as the signed release.
