# Build the Windows installers for Mi Band Bridge and Mi Band App Editor (W12), and
# copy them to dist\windows with their release names. Run it on Windows, in
# Windows PowerShell:
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-windows.ps1
#
# Each installer is Tauri's NSIS installer, for the current user, with no
# administrator prompt (W12).
#
# The build maps this PC's paths out of the binaries. Before it names the
# installers, scripts\scan-windows-binaries.ps1 scans what they carry for
# private details, and a hit stops the build.
#
# The installers come from Tauri 2's command line tool, pinned to the Tauri
# the apps build with. The first run builds that tool into target\tauri-cli,
# which takes a few minutes. It leaves any other copy of the tool alone,
# such as a Tauri 1 cargo-tauri.
#
# Build in a folder whose path has no space, as every Windows build of this
# repo does (the Windows port plan's rule).

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
if ($repo -match ' ') {
    throw "Build in a folder whose path has no space. This one is $repo."
}

$cliRoot = Join-Path $repo 'target\tauri-cli'
$cli = Join-Path $cliRoot 'bin\cargo-tauri.exe'
if (-not (Test-Path $cli)) {
    Write-Host 'Building the Tauri 2 command line tool into target\tauri-cli...'
    cargo install tauri-cli --version '~2.11' --locked --root $cliRoot
    if ($LASTEXITCODE -ne 0) { throw 'cargo could not build the Tauri command line tool.' }
}

# A build writes the paths of this PC into the binaries, through panic
# messages and file!(): the cargo registry's sources, the toolchain's, and
# this checkout's. Each is mapped to a neutral name, from the environment,
# so this script names no person. The home folder goes first, as a fallback:
# rustc uses the last mapping that matches. CARGO_ENCODED_RUSTFLAGS takes
# paths with spaces, which RUSTFLAGS cannot.
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
$env:CARGO_ENCODED_RUSTFLAGS = @(
    "--remap-path-prefix=$env:USERPROFILE=/home",
    "--remap-path-prefix=$cargoHome=/cargo",
    "--remap-path-prefix=$rustupHome=/rustup",
    "--remap-path-prefix=$repo=/build"
) -join [char]0x1f
# The C code in some crates, such as aws-lc-sys under rustls, writes its own
# source paths through __FILE__, and the mappings above reach only rustc. So
# the apps build with a cargo home inside this checkout's target folder, a
# path that names no person. Cargo fetches the locked crates there once.
$env:CARGO_HOME = Join-Path $repo 'target\cargo-home'

$bundles = Join-Path $repo 'target\release\bundle\nsis'
Remove-Item -Recurse -Force $bundles -ErrorAction SilentlyContinue
foreach ($app in 'gui', 'editor') {
    Push-Location (Join-Path $repo $app)
    try {
        Write-Host "Building the $app installer..."
        & $cli build
        if ($LASTEXITCODE -ne 0) { throw "The $app installer did not build." }
    } finally {
        Pop-Location
    }
}

$dist = Join-Path $repo 'dist\windows'
New-Item -ItemType Directory -Force $dist | Out-Null
$made = @(Get-ChildItem $bundles -Filter '*-setup.exe')
if ($made.Count -ne 2) {
    throw "Expected 2 installers in $bundles, and found $($made.Count)."
}
# Before the installers get their names, scan every .exe and .dll they carry
# for private details: the two apps, and the NSIS plugins of Tauri's
# installer. scan-windows-binaries.ps1 holds the patterns.
$carried = @(
    (Join-Path $repo 'target\release\bandbridge-gui.exe'),
    (Join-Path $repo 'target\release\bandbridge-editor.exe'),
    (Join-Path $env:LOCALAPPDATA 'tauri\NSIS\Plugins\x86-unicode')
)
& powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'scan-windows-binaries.ps1') @carried
if ($LASTEXITCODE -ne 0) {
    throw 'A binary the installers carry holds private details, so the installers were not named.'
}
# Tauri names an installer "<product name>_<version>_x64-setup.exe", and has
# no setting for it. The release names have no space, and name the system:
# Mi-Band-Bridge-1.0.0-windows-x64-setup.exe. The installers of an earlier
# build go first, so dist\windows holds this build's two and no others.
Get-ChildItem $dist -Filter '*-setup.exe' | Remove-Item -Force
foreach ($installer in $made) {
    if ($installer.Name -notmatch '^(?<name>.+)_(?<version>[^_]+)_x64-setup\.exe$') {
        throw "Tauri named an installer $($installer.Name), not <name>_<version>_x64-setup.exe."
    }
    $release = "$($Matches.name -replace ' ', '-')-$($Matches.version)-windows-x64-setup.exe"
    Copy-Item $installer.FullName (Join-Path $dist $release) -Force
    Write-Host "dist\windows\$release"
}
