# Local Windows build

Prerequisites: Rust with the nightly toolchain, Visual Studio 2022 Build Tools
with the C++ build tools workload and Windows SDK. GitHub Actions is not needed.

From the repository folder:

```powershell
# Fast incremental debug build (also available by double-clicking Build local.cmd).
.\build-local.ps1

# Tests and an optimized build.
.\build-local.ps1 -Mode Release -Test

# Build, test, and replace the existing animation preview installation.
.\build-local.ps1 -Mode Release -Test -Install
```

If PowerShell's execution policy prevents starting the script, use the wrapper:

```bat
"Build local.cmd" -Mode Release -Test -Install
```

Outputs: `target/debug` or `target/release`, containing `glazewm.exe`,
`glazewm-cli.exe`, and `glazewm-watcher.exe`. Cargo caches unchanged dependencies.
The first build downloads dependencies; later builds reuse the local cache.

`-Install` targets only `%LOCALAPPDATA%\Programs\GlazeWM-Animations`, keeps a
timestamped backup there, restarts the preview, and checks that its CLI responds.
It does not edit the GlazeWM configuration or startup registration. Exit another
GlazeWM installation before using it. Test failures stop installation.
