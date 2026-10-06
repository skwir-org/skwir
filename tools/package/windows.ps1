# Package bibli for Windows (Phase 5, step K): build what ships, then the
# installer from tools\package\windows\bibli.wxs. Run on Windows with the
# Rust toolchain and WiX 5 or later (`dotnet tool install --global wix`);
# written on Linux, never run yet (ADR-035: 5b runs it).
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
# The helpers an installer carries (ADR-041): Mermaid and MuPDF are left
# out, as Linux's packages only suggest them; those linking native
# libraries (GStreamer, libarchive) wait for their libraries here.
cargo build --profile dist -p bibli -p bibli-pdf-worker -p bibli-model-worker
# PDFium, beside the PDF helper, where it finds it (ADR-041): the release
# tools\package\pdfium.lock pins, checked against its sum.
$lock = Get-Content tools\package\pdfium.lock | Where-Object { $_ -notmatch '^#' }
$release = ($lock | Where-Object { $_ -match '^release ' }) -replace '^release ', ''
$platform = if ($env:PROCESSOR_ARCHITECTURE -eq "ARM64") { "win-arm64" } else { "win-x64" }
$asset = "pdfium-$platform.tgz"
$sum = (($lock | Where-Object { $_.StartsWith("$asset ") }) -split ' ')[1]
$download = "target\$asset"
Invoke-WebRequest -Uri "https://github.com/bblanchon/pdfium-binaries/releases/download/$($release -replace '/', '%2F')/$asset" -OutFile $download
if ((Get-FileHash -Algorithm SHA256 $download).Hash.ToLower() -ne $sum) {
    throw "${asset}: its sum is not the one pinned"
}
$unpacked = "target\pdfium-$platform"
if (Test-Path $unpacked) { Remove-Item -Recurse -Force $unpacked }
New-Item -ItemType Directory -Force $unpacked | Out-Null
tar -xzf $download -C $unpacked
Copy-Item "$unpacked\bin\pdfium.dll" target\dist\
$licenses = "target\dist\licenses"
if (Test-Path $licenses) { Remove-Item -Recurse -Force $licenses }
New-Item -ItemType Directory -Force "$licenses\pdfium" | Out-Null
Copy-Item "$unpacked\LICENSE", "$unpacked\licenses\*" "$licenses\pdfium"
Copy-Item LICENSE $licenses
# Beside bibli.exe, where it looks for it.
$share = "target\dist\share\mime"
if (Test-Path $share) { Remove-Item -Recurse -Force $share }
New-Item -ItemType Directory -Force $share | Out-Null
Copy-Item "$MimeDatabase\*" $share
New-Item -ItemType Directory -Force target\package | Out-Null
wix build tools\package\windows\bibli.wxs -d "Version=$Version" -d "Dist=$root\target\dist" -o "target\package\bibli-$Version.msi"
Write-Output "target\package\bibli-$Version.msi"
