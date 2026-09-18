<div align="center">

> Version 4.0.4 is available: [download the Windows x64 release](https://github.com/mixadze1/glazewm_reborn/releases/latest).

  <br>
  <img src="./resources/assets/logo.svg" width="230" alt="GlazeWM logo" />
  <br>

# GlazeWM Reborn

**The idea behind Reborn: give window management a better feel.**

Reborn is a Windows-focused fork of GlazeWM built around the way your desktop **moves, responds, and feels**. Workspace transitions carry their motion through rapid direction changes. Windows travel toward their destination. Resize controls follow your held keys, and focus outlines make the current mode visible. The goal of version 4 is to make everyday tiling feel more connected and responsive.

[![Downloads][downloads-badge]][downloads-link]
[![Issues][issues-badge]][issues-link]

Built on [GlazeWM by glzr-io](https://github.com/glzr-io/glazewm), the keyboard-driven tiling window manager inspired by i3wm. Reborn keeps its YAML configuration and keyboard workflow while adding motion, visual feedback, and layout refinements. The executable and configuration names remain compatible with GlazeWM.

[Installation](#installation) •
[Version 4 history](#version-4-history) •
[Default keybindings](#default-keybindings) •
[Config documentation](#config-documentation) •
[FAQ](#faq) •
[Contributing](CONTRIBUTING.md)

![GlazeWM Reborn 4 demo: workspace motion, keyboard resizing, and focus outlines][demo-video]

</div>

### 🌟 Key features

- Simple YAML configuration
- Multi-monitor support
- Customizable rules for specific windows
- Portable Windows x64 release with no console window for the application or watcher
- Integration with [Zebar](https://github.com/glzr-io/zebar) as a status bar

### What's new in Reborn

- Workspace and window motion is part of the interaction: transitions preserve continuity when you change direction or choose another destination mid-animation.
- Focus outlines follow rounded window corners with antialiased edges and configurable thickness.
- `Alt + R` enters resize mode with a configurable highlight color (yellow in the example config). Holding arrows or HJKL changes size in small frame-paced steps without queuing autorepeats; releasing the key stops generating resize steps. The configured resize step defaults to 2% and controls speed.
- `Alt + V` chooses the direction for the next tiled window. Automatic placement no longer overrides that direction or moves the new window into a different group to accommodate minimum sizes.
- An `Active` tray checkmark toggles tiling and stays in sync with `Alt + Shift + P`.
- Pausing removes both the custom outline and the native colored border; resuming restores the configured colors. The tray shows your configured pause shortcut.
- Automatic safe areas on Windows follow visible Zebar edge bars and the
  Windows taskbar. Showing, hiding, closing, or resizing a bar updates the
  tiling area within approximately 500 ms. Configured outer gaps remain
  outside the windows, measured from the safe area; Windows appbar space
  is not counted twice. Zebar panels must span at least half the monitor,
  be thin (at most a quarter of its depth), and sit within 64 logical px
  of an edge. Small widgets and central overlays do not reserve space.
- Smooth workspace slides that preserve window positions and motion when changing destination rapidly.
- Visible windows on intermediate workspaces when travelling across several desktops.
- Directional window transfers: exit toward the destination, then reveal at the new layout position.
- Consistent sizing when moving tiled windows with directional keybindings.
- Configurable focused-window outline and keyboard resize limits.
- Configurable animation durations and easing curves.

## Version 4 history

Version 4 is the Reborn line: a series of changes focused on the **feeling of using a tiling desktop**, from movement between workspaces to the feedback around a single window.

| Release | What changed |
| --- | --- |
| [4.0.0 — The Reborn foundation](https://github.com/mixadze1/glazewm_reborn/releases/tag/v4.0.0) | Animated workspace slides with continuous position and velocity when retargeting or reversing; visible intermediate workspaces; directional window transfers with an exit and reveal; configurable duration and easing. Also improved live mouse resizing, minimum-size handling, directional move sizing, and focused-window outlines. |
| [4.0.2 — Clear active state](https://github.com/mixadze1/glazewm_reborn/releases/tag/v4.0.2) | Added the Active tray checkmark, synchronized with the pause shortcut, so tiling can be toggled from either the keyboard or the tray. |
| [4.0.3 — A desktop that adapts](https://github.com/mixadze1/glazewm_reborn/releases/tag/v4.0.3) | Added automatic safe areas for visible edge bars and the Windows taskbar. The layout adapts as panels appear, disappear, or change size, while preserving configured outer gaps. |
| [4.0.4 — More direct control](https://github.com/mixadze1/glazewm_reborn/releases/tag/v4.0.4) | Added rounded, antialiased outlines and a separate resize-mode color; removed borders while paused; preserved the selected tiling direction; replaced queued resize autorepeats with held-key updates that keep the outline visible. The default resize step is 2%. |

## Installation

1. Download `glazewm-v4.0.4-windows-x64.zip` from the [Reborn releases](https://github.com/mixadze1/glazewm_reborn/releases/latest).
2. Extract the entire archive into one directory.
3. Close any existing GlazeWM instance, then launch `glazewm.exe`.

Keep `glazewm-watcher.exe` beside the main executable. Both run without a console window; `glazewm-cli.exe` is the command-line companion. The release includes `config.example.yaml`, a license, and SHA-256 checksums. The portable build is unsigned and does not include an installer or Zebar.

Existing configuration is read from `%USERPROFILE%\.glzr\glazewm\config.yaml`. Back it up before adopting settings from the example. Package-manager packages named `GlazeWM` or `glazewm` belong to the upstream project and do not install this fork.

### Build from source

Use the Rust toolchain specified by this repository and the Windows C++ build tools. From PowerShell:

```powershell
$env:VERSION_NUMBER = '4.0.4'
cargo build --release -p wm -p wm-cli -p wm-watcher
```

The executables are written to `target/release`. The main application and watcher use the Windows GUI subsystem in release builds.

## Contributing

Help fix something that annoys you, or add a feature you've been wanting for a long time! Contributions are very welcome.

Local development and guidelines are available in the [contributing guide](CONTRIBUTING.md). Report fork-specific problems in [this repository's issues](https://github.com/mixadze1/glazewm_reborn/issues).

## Default keybindings

On the first launch of GlazeWM, a default configuration can optionally be generated.

The [sample configuration](resources/assets/sample-config.yaml) is the source of truth for this fork's default bindings. The inherited cheat sheet below is a general reference; your own configuration can override it.

![Infographic](/resources/assets/cheatsheet.png)

## Config documentation

The [default config](resources/assets/sample-config.yaml) file is generated at `%userprofile%\.glzr\glazewm\config.yaml`.

### Reborn animations

Merge these settings into your existing configuration rather than replacing it:

```yaml
animations:
  workspace_switch:
    enabled: true
    duration_ms: 450
    easing: ease_in_out
    style: slide
    direction: horizontal
    zoom_factor: 0.0
  window_open:
    enabled: true
    duration_ms: 250
    easing: ease_in_out_cubic
```

Workspace slides use `workspace_switch.duration_ms` and `easing`. For a window transfer followed by workspace focus, the exit uses this timing, then the centered reveal uses `window_open.duration_ms` and `easing`. Set `window_open.enabled: false` to keep only the exit. The reveal is centered regardless of the style selected for ordinary new-window animations. Named easing curves and `cubic_bezier(x1, y1, x2, y2)` are supported; see the sample configuration for all options.

To move the focused window and follow it, bind both commands together:

```yaml
keybindings:
  - commands: ["move --workspace 2", "focus --workspace 2"]
    bindings: ["alt+shift+2"]
```

The focused outline is controlled by `window_effects.focused_border_width`. Set `window_effects.resize_border_color` to change its color during `Alt + R`; leaving this setting out keeps the regular focused color. Exiting resize mode restores the regular color, and pausing tiling clears both borders.

```yaml
window_effects:
  focused_border_width: 3
  resize_border_color: "#ffff00"
```

Resize speed is controlled by the `resize` commands under `binding_modes` in your configuration. The default is `resize --width +2%` (with matching negative-width and positive/negative-height bindings). Use 1% for half that speed, or 4% for twice the speed. While a key is held, the configured step is divided into smaller updates on a 16 ms timer; missed frames are skipped rather than replayed after release. This timing is independent of window animation duration.

Keyboard resize limits are controlled by `window_behavior.resize_respects_minimum_size`; these are separate from mouse resize constraints.

To use a different config file location, you can launch the GlazeWM executable with the CLI argument `--config="..."`, like so:

```sh
./glazewm.exe start --config="C:\<PATH_TO_CONFIG>\config.yaml"
```

Or pass a value for the `GLAZEWM_CONFIG_PATH` environment variable:

```sh
setx GLAZEWM_CONFIG_PATH "C:\<PATH_TO_CONFIG>\config.yaml"
```

With the benefit of using a custom path being that you can choose a different name for the config file, such as `glazewm.yaml`.

### Config: General

```yaml
general:
  # Commands to run when the WM has started (e.g. to run a script or launch
  # another application).
  startup_commands: []

  # Commands to run just before the WM is shutdown.
  shutdown_commands: []

  # Commands to run after the WM config has reloaded.
  config_reload_commands: []

  # Whether to automatically focus windows underneath the cursor.
  focus_follows_cursor: false

  # Whether to switch back and forth between the previously focused
  # workspace when focusing the current workspace.
  toggle_workspace_on_refocus: false

  cursor_jump:
    # Whether to automatically move the cursor on the specified trigger.
    enabled: true

    # Trigger for cursor jump:
    # - 'monitor_focus': Jump when focus changes between monitors.
    # - 'window_focus': Jump when focus changes between windows.
    trigger: "monitor_focus"
```

### Config: Keybindings

The available keyboard shortcuts can be customized via the `keybindings` option. A keybinding consists of one or more key combinations and one or more commands to run when pressed.

It's recommended to use the alt key for keybindings. The Windows key is unfortunately a pain to remap, since the OS reserves certain keybindings (e.g. `lwin+l`).

```yaml
keybindings:
  # Command(s) to run.
  - commands: ["focus --workspace 1"]

    # Key combination(s) to trigger the keybinding.
    bindings: ["alt+1"]

  # Multiple commands can be run in a sequence (e.g. to move a window to a
  # workspace + focus workspace).
  - commands: ["move --workspace 1", "focus --workspace 1"]
    bindings: ["alt+shift+1"]
```

**Full list of keys that can be used for keybindings:**

<details>
<summary>Keys list</summary>

| Key                   | Description                                                               |
| --------------------- | ------------------------------------------------------------------------- |
| `a` - `z`             | Alphabetical letter keys                                                  |
| `0` - `9`             | Number keys                                                               |
| `numpad0` - `numpad9` | Numerical keypad keys                                                     |
| `f1` - `f24`          | Function keys                                                             |
| `shift`               | Either left or right SHIFT key                                            |
| `lshift`              | The left SHIFT key                                                        |
| `rshift`              | The right SHIFT key                                                       |
| `control`             | Either left or right CTRL key                                             |
| `lctrl`               | The left CTRL key                                                         |
| `rctrl`               | The right CTRL key                                                        |
| `alt`                 | Either left or right ALT key                                              |
| `lalt`                | The left ALT key                                                          |
| `ralt`                | The right ALT key                                                         |
| `lwin`                | The left ⊞ Windows logo key                                               |
| `rwin`                | The right ⊞ Windows logo key                                              |
| `space`               | The spacebar key                                                          |
| `escape`              | The ESCAPE key                                                            |
| `back`                | The BACKSPACE key                                                         |
| `tab`                 | The TAB key                                                               |
| `enter`               | The ENTER key                                                             |
| `left`                | The ← arrow key                                                           |
| `right`               | The → arrow key                                                           |
| `up`                  | The ↑ arrow key                                                           |
| `down`                | The ↓ arrow key                                                           |
| `num_lock`            | The NUM LOCK key                                                          |
| `scroll_lock`         | The SCROLL LOCK key                                                       |
| `caps_lock`           | The CAPS LOCK key                                                         |
| `page_up`             | The PAGE UP key                                                           |
| `page_down`           | The PAGE DOWN key                                                         |
| `insert`              | The INSERT key                                                            |
| `delete`              | The DELETE key                                                            |
| `end`                 | The END key                                                               |
| `home`                | The HOME key                                                              |
| `print_screen`        | The PRINT SCREEN key                                                      |
| `multiply`            | The `*` key (only on numpad)                                              |
| `add`                 | The `+` key (only on numpad)                                              |
| `subtract`            | The `-` key (only on numpad)                                              |
| `decimal`             | The DEL key (only on numpad)                                              |
| `divide`              | The `/` key (only on numpad)                                              |
| `volume_up`           | The volume up key                                                         |
| `volume_down`         | The volume down key                                                       |
| `volume_mute`         | The volume mute key                                                       |
| `media_next_track`    | The media next track key                                                  |
| `media_prev_track`    | The media prev track key                                                  |
| `media_stop`          | The media stop key                                                        |
| `media_play_pause`    | The media play/pause key                                                  |
| `oem_semicolon`       | The `;`/`:` key on a US standard keyboard (varies by keyboard)            |
| `oem_question`        | The `/`/`?` key on a US standard keyboard (varies by keyboard)            |
| `oem_tilde`           | The `` ` ``/`~` key on a US standard keyboard (varies by keyboard)        |
| `oem_open_brackets`   | The `[`/`{` key on a US standard keyboard (varies by keyboard)            |
| `oem_pipe`            | The `\`/`\|` key on a US standard keyboard (varies by keyboard)           |
| `oem_close_brackets`  | The `]`/`}` key on a US standard keyboard (varies by keyboard)            |
| `oem_quotes`          | The `'`/`"` key on a US standard keyboard (varies by keyboard)            |
| `oem_8`               | The `` ` ``/`¬` key on a UK keyboard (varies by keyboard)                 |
| `oem_102`             | The `\`/`\|` key next to left Shift on ISO keyboards (varies by keyboard) |
| `oem_plus`            | The `=`/`+` key on a US standard keyboard (varies by keyboard)            |
| `oem_comma`           | The `,`/`<` key on a US standard keyboard (varies by keyboard)            |
| `oem_minus`           | The `-`/`_` key on a US standard keyboard (varies by keyboard)            |
| `oem_period`          | The `.`/`>` key on a US standard keyboard (varies by keyboard)            |
| `muhenkan`            | The 無変換 (non-convert) key for Japanese keyboard layouts                |
| `henkan`              | The 変換 (convert) key for Japanese keyboard layouts                      |

</details>

If a key is not in the list above, it is likely still supported if you use its character in a keybinding (e.g. `alt+å` for the Norwegian Å character).

> German and US international keyboards treat the right-side alt key differently. For these keyboard layouts, use `ralt+ctrl` instead of `ralt` to bind the right-side alt key.

### Config: Gaps

The gaps between windows can be changed via the `gaps` property in the config file. Inner and outer gaps are set separately.

```yaml
gaps:
  # Gap between adjacent windows.
  inner_gap: "20px"

  # Gap between windows and the screen edge.
  outer_gap:
    top: "20px"
    right: "20px"
    bottom: "20px"
    left: "20px"
```

### Config: Workspaces

Workspaces need to be predefined via the `workspaces` property in the config file. A workspace is automatically assigned to each monitor on startup.

```yaml
workspaces:
  # This is the unique ID for the workspace. It's used in keybinding
  # commands, and is also the label shown in 3rd-party apps (e.g. Zebar) if
  # `display_name` is not provided.
  - name: "1"

    # Optional override for the workspace label used in 3rd-party apps.
    # Does not need to be unique.
    display_name: "Work"

    # Optionally force the workspace on a specific monitor if it exists.
    # 0 is your leftmost screen, 1 is the next one to the right, and so on.
    bind_to_monitor: 0

    # Optionally prevent workspace from being deactivated when empty.
    keep_alive: false
```

### Config: Window rules

Commands can be run when a window is first launched. This is useful for adding window-specific behaviors like always starting a window as fullscreen or assigning to a specific workspace.

Windows can be targeted by their process, class, and title. Multiple matching criteria can be used together to target a window more precisely.

```yaml
window_rules:
  - commands: ["move --workspace 1"]
    match:
      # Move browsers to workspace 1.
      - window_process: { regex: "msedge|brave|chrome" }

  - commands: ["ignore"]
    match:
      # Ignores any Zebar windows.
      - window_process: { equals: "zebar" }

      # Ignores picture-in-picture windows for browsers.
      # Note that *both* the title and class must match for the rule to run.
      - window_title: { regex: "[Pp]icture.in.[Pp]icture" }
        window_class: { regex: "Chrome_WidgetWin_1|MozillaDialogClass" }
```

### Config: Window effects

Visual effects can be applied to windows via the `window_effects` option. Currently, colored borders are the only effect available with more to come in the future.

> Note: Window effects are exclusive to Windows 11.

```yaml
window_effects:
  # Visual effects to apply to the focused window.
  focused_window:
    # Highlight the window with a colored border.
    border:
      enabled: true
      color: "#0000ff"

  # Visual effects to apply to non-focused windows.
  other_windows:
    border:
      enabled: false
      color: "#d3d3d3"
```

### Config: Window behavior

The `window_behavior` config option exists to customize the states that a window can be in (`tiling`, `floating`, `minimized`, and `fullscreen`).

```yaml
window_behavior:
  # New windows are created in this state whenever possible.
  # Allowed values: 'tiling', 'floating'.
  initial_state: "tiling"

  # Sets the default options for when a new window is created. This also
  # changes the defaults for when the state change commands, like
  # `set-floating`, are used without any flags.
  state_defaults:
    floating:
      # Whether to center floating windows by default.
      centered: true

      # Whether to show floating windows as always on top.
      shown_on_top: false

    fullscreen:
      # Maximize the window if possible. If the window doesn't have a
      # maximize button, then it'll be made fullscreen normally instead.
      maximized: false
```

### Config: Binding modes

Binding modes are used to modify keybindings while GlazeWM is running.

A binding mode can be enabled with `wm-enable-binding-mode --name <NAME>` and disabled with `wm-disable-binding-mode --name <NAME>`.

```yaml
binding_modes:
  # When enabled, the focused window can be resized via arrow keys or HJKL.
  - name: "resize"
    keybindings:
      - commands: ["resize --width -2%"]
        bindings: ["h", "left"]
      - commands: ["resize --width +2%"]
        bindings: ["l", "right"]
      - commands: ["resize --height +2%"]
        bindings: ["k", "up"]
      - commands: ["resize --height -2%"]
        bindings: ["j", "down"]
      # Press enter/escape to return to default keybindings.
      - commands: ["wm-disable-binding-mode --name resize"]
        bindings: ["escape", "enter"]
```

## FAQ

**Q: How do I run GlazeWM on startup?**

Right-click the GlazeWM icon in the system tray and select "Run on system startup".

**Q: How can I create `<insert layout>`?**

You can create custom layouts by changing the tiling direction with `alt+v`. This changes where the next window is placed _in relation to the current window_. If the current window's direction is horizontal, the new window will be placed to the right of it. If it is vertical, it will be placed below it. This also applies when moving windows; the tiling direction of the stationary window will affect where the moved window will be placed.

Community-made scripts like [Dutch-Raptor/GAT-GWM](https://github.com/Dutch-Raptor/GAT-GWM) and [burgr033/GlazeWM-autotiling-python](https://github.com/burgr033/GlazeWM-autotiling-python) can be used to automatically change the tiling direction. Native support for automatic layouts isn't _currently_ supported.

**Q: How do I create a rule for `<insert application>`?**

To match a specific application, you need a command to execute and either the window's process name, title, or class name. For example, if you use Flow-Launcher and want to make the settings window float, you can do the following:

```yaml
window_rules:
  - commands: ["set-floating"]
    match:
      - window_process: { equals: "Flow.Launcher" }
        window_title: { equals: "Settings" }
```

Programs like Winlister or AutoHotkey's Window Spy can be useful for getting info about a window.

**Q: How can I ignore GlazeWM's keybindings when `<insert application>` is focused?**

This isn't currently supported, however, the keybinding `alt+shift+p` in the default config is used to disable all other keybindings until `alt+shift+p` is pressed again.

## Credits and license

This is a fork of [glzr-io/glazewm](https://github.com/glzr-io/glazewm). Credit for the original application, logo, demo, and inherited documentation belongs to the upstream authors and contributors. Distributed under [GPL-3.0](LICENSE.md).

[downloads-badge]: https://img.shields.io/github/downloads/mixadze1/glazewm_reborn/total?logo=github&logoColor=white
[downloads-link]: https://github.com/mixadze1/glazewm_reborn/releases
[issues-badge]: https://img.shields.io/github/issues/mixadze1/glazewm_reborn
[issues-link]: https://github.com/mixadze1/glazewm_reborn/issues
[demo-video]: resources/assets/reborn-demo.gif
