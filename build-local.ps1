[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release')]
    [string]$Mode = 'Debug',
    [switch]$Test,
    [switch]$Install,
    [string]$Version = '4.0.4'
)

$ErrorActionPreference = 'Stop'
$cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (!(Test-Path -LiteralPath $cargo)) { throw 'Install Rust using rustup first.' }
if (!(Test-Path -LiteralPath $vswhere)) { throw 'Install Visual Studio Build Tools with the C++ build tools workload first.' }
$vsPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$vsPath) { throw 'The MSVC x64 toolchain is missing. Add the C++ build tools workload in Visual Studio Installer.' }
Import-Module (Join-Path $vsPath 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $vsPath -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null

$oldVersion = $env:VERSION_NUMBER
$env:VERSION_NUMBER = $Version
Push-Location $PSScriptRoot
try {
    if ($Test) {
        & $cargo +nightly test --locked --workspace
        if ($LASTEXITCODE -ne 0) { throw "Tests failed ($LASTEXITCODE)." }
    }
    $buildArgs = @('+nightly', 'build', '--locked', '--workspace')
    if ($Mode -eq 'Release') { $buildArgs += '--release' }
    & $cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw "Build failed ($LASTEXITCODE)." }
    $outputDir = Join-Path $PSScriptRoot ('target\' + $Mode.ToLowerInvariant())
    $binaries = @('glazewm.exe', 'glazewm-cli.exe', 'glazewm-watcher.exe')
    foreach ($name in $binaries) {
        if (!(Test-Path -LiteralPath (Join-Path $outputDir $name))) { throw "Missing build output: $name" }
    }
    Write-Host "Build ready: $outputDir"

    if ($Install) {
        $installDir = Join-Path $env:LOCALAPPDATA 'Programs\GlazeWM-Animations'
        $installedExe = Join-Path $installDir 'glazewm.exe'
        $installedCli = Join-Path $installDir 'glazewm-cli.exe'
        if (!(Test-Path -LiteralPath $installedExe)) {
            throw 'The animation preview installation was not found. Build outputs are ready; installation was skipped.'
        }
        $foreignInstance = Get-Process -Name glazewm -ErrorAction SilentlyContinue | Where-Object { $_.Path -ne $installedExe }
        if ($foreignInstance) { throw 'A different GlazeWM installation is running. Exit it before installing this preview.' }
        $backupDir = Join-Path $installDir ('backups\' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
        New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
        foreach ($name in ($binaries + 'BUILD.txt')) {
            $source = Join-Path $installDir $name
            if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination $backupDir }
        }
        $ownedProcesses = Get-Process -Name glazewm,glazewm-watcher -ErrorAction SilentlyContinue | Where-Object {
            $_.Path -eq $installedExe -or $_.Path -eq (Join-Path $installDir 'glazewm-watcher.exe')
        }
        if ($ownedProcesses) {
            & $installedCli command wm-exit
            foreach ($process in $ownedProcesses) {
                if (!$process.WaitForExit(5000)) { Stop-Process -Id $process.Id }
            }
        }
        try {
            foreach ($name in $binaries) { Copy-Item -LiteralPath (Join-Path $outputDir $name) -Destination $installDir -Force }
            $commit = & git rev-parse HEAD
            @("Local $Mode build", "Commit: $commit", "Built: $(Get-Date -Format o)", "Backup: $backupDir") | Set-Content -LiteralPath (Join-Path $installDir 'BUILD.txt')
            Start-Process -FilePath $installedExe -WindowStyle Hidden
            Start-Sleep -Seconds 2
            $healthJson = & $installedCli query app-metadata
            if ($LASTEXITCODE -ne 0) { throw 'Installed GlazeWM did not answer the health check.' }
            $health = $healthJson | ConvertFrom-Json
            if (!$health.success) { throw "Installed GlazeWM health check failed: $($health.error)" }
            Write-Host $healthJson
            Write-Host "Installed. Previous binaries: $backupDir"
        } catch {
            Get-Process -Name glazewm,glazewm-watcher -ErrorAction SilentlyContinue | Where-Object {
                $_.Path -eq $installedExe -or $_.Path -eq (Join-Path $installDir 'glazewm-watcher.exe')
            } | Stop-Process
            foreach ($name in ($binaries + 'BUILD.txt')) {
                $backup = Join-Path $backupDir $name
                if (Test-Path -LiteralPath $backup) { Copy-Item -LiteralPath $backup -Destination $installDir -Force }
            }
            Start-Process -FilePath $installedExe -WindowStyle Hidden
            throw
        }
    }
} finally {
    Pop-Location
    $env:VERSION_NUMBER = $oldVersion
}
