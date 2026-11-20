# Builds GeminiDesk Windows release packages (Portable ZIP + NSIS Per-User Installer)
param(
    [string]$Version = "0.4.0",
    [string]$BinaryPath = "",
    [string]$MakeNsisPath = "",
    [string]$OutDir = "",
    [switch]$RequireNsis
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $BinaryPath) { $BinaryPath = Join-Path $RepoRoot "target\release\gemini-desk.exe" }
if (-not $OutDir) { $OutDir = Join-Path $RepoRoot "dist" }

if (-not (Test-Path $BinaryPath)) {
    throw "Release binary not found at $BinaryPath. Run 'cargo build --release' first."
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# 1. Portable ZIP package (uses standard %LOCALAPPDATA% / %APPDATA% profile location)
$StageDir = Join-Path $OutDir "portable-stage"
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

Copy-Item $BinaryPath (Join-Path $StageDir "gemini-desk.exe")
Copy-Item (Join-Path $RepoRoot "README.md") (Join-Path $StageDir "README.md")
Copy-Item (Join-Path $RepoRoot "THIRD_PARTY_LICENSES.md") (Join-Path $StageDir "THIRD_PARTY_LICENSES.md")

$ZipPath = Join-Path $OutDir "GeminiDesk-$Version-Windows-x64-Portable.zip"
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ZipPath -CompressionLevel Optimal
Remove-Item -Recurse -Force $StageDir
Write-Output ("Built Portable ZIP: {0} ({1:N0} bytes)" -f $ZipPath, (Get-Item $ZipPath).Length)

# 2. NSIS Per-User Installer
if (-not $MakeNsisPath) {
    $cmd = Get-Command makensis -ErrorAction SilentlyContinue
    if ($cmd) {
        $MakeNsisPath = $cmd.Source
    } elseif (Test-Path "C:\Program Files (x86)\NSIS\makensis.exe") {
        $MakeNsisPath = "C:\Program Files (x86)\NSIS\makensis.exe"
    } elseif (Test-Path "C:\Program Files\NSIS\makensis.exe") {
        $MakeNsisPath = "C:\Program Files\NSIS\makensis.exe"
    }
}

if ($MakeNsisPath -and (Test-Path $MakeNsisPath)) {
    $SetupExe = Join-Path $OutDir "GeminiDesk-$Version-Windows-x64-Setup.exe"
    $NsiFile = Join-Path $PSScriptRoot "geminidesk.nsi"
    & $MakeNsisPath "/DPRODUCT_VERSION=$Version" "/DBINARY_PATH=$BinaryPath" "/DOUT_FILE=$SetupExe" $NsiFile
    if ($LASTEXITCODE -ne 0) {
        throw "makensis failed with exit code $LASTEXITCODE"
    }
    Write-Output ("Built NSIS Installer: {0} ({1:N0} bytes)" -f $SetupExe, (Get-Item $SetupExe).Length)
} elseif ($RequireNsis) {
    throw "makensis not found and -RequireNsis was specified."
} else {
    Write-Warning "makensis not found; skipped NSIS installer compilation (Portable ZIP was created)."
}
