# Package bibli for Windows (Phase 5, step K): build what ships, then the
# installer from tools\package\windows\bibli.wxs. Run on Windows with the
# Rust toolchain and WiX (`dotnet tool install --global wix`); written on
# Linux, never run yet (ADR-035: 5b runs it).
#
#     powershell -File tools\package\windows.ps1 -MimeDatabase <dir> [-Version 0.0.1]
#
# <dir> is the type database, the fallback of the system's types
# (ADR-037), as tools/package/mime-database.sh compiles it from
# vendor/shared-mime-info on Linux or a Mac: Windows has no
# update-mime-database to compile it with.
param(
    [Parameter(Mandatory = $true)][string]$MimeDatabase,
    [string]$Version = "0.0.1"
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot\..\.."
Set-Location $root
if (-not (Test-Path "$MimeDatabase\globs2")) {
    throw "${MimeDatabase}: no type database (tools/package/mime-database.sh makes one)"
}
# The helpers linking native libraries (Poppler, MuPDF, GStreamer,
# libarchive) need those libraries beside them: not done yet.
cargo build --profile dist -p bibli -p bibli-pdf-worker -p bibli-mermaid-worker -p bibli-model-worker
# Beside bibli.exe, where it looks for it.
$share = "target\dist\share\mime"
if (Test-Path $share) { Remove-Item -Recurse -Force $share }
New-Item -ItemType Directory -Force $share | Out-Null
Copy-Item "$MimeDatabase\*" $share
New-Item -ItemType Directory -Force target\package | Out-Null
wix build tools\package\windows\bibli.wxs -d "Version=$Version" -d "Dist=$root\target\dist" -o "target\package\bibli-$Version.msi"
Write-Output "target\package\bibli-$Version.msi"
