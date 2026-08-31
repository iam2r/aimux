# One-line install (PowerShell):
#   irm https://github.com/iam2r/apmux/releases/latest/download/install.ps1 | iex
$ErrorActionPreference = "Stop"

$Repo = if ($env:APMUX_REPO) { $env:APMUX_REPO } else { "iam2r/apmux" }
$InstallDir = if ($env:APMUX_INSTALL_DIR) { $env:APMUX_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "apmux\bin" }
$SkipPath = $env:APMUX_SKIP_PATH -eq "1"
$Version = if ($args.Count -ge 1 -and $args[0]) { $args[0] } else { "latest" }
if ($Version -ne "latest" -and $Version -notmatch '^v') { $Version = "v$Version" }

$Asset = "apmux-windows-x64.zip"
$Releases = "https://github.com/$Repo/releases"
$Url = if ($Version -eq "latest") {
    "$Releases/latest/download/$Asset"
} else {
    "$Releases/download/$Version/$Asset"
}
$Target = Join-Path $InstallDir "apmux.exe"

function Write-Info($msg) { Write-Host "  info: $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "  warn: $msg" -ForegroundColor Yellow }
function Write-Err($msg) { Write-Host "  error: $msg" -ForegroundColor Red }

$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("apmux-install-" + [guid]::NewGuid().ToString("n"))
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    Write-Info "Downloading $Asset"
    $zip = Join-Path $Tmp $Asset
    Invoke-WebRequest -Uri $Url -OutFile $zip -UseBasicParsing

    Write-Info "Extracting archive"
    Expand-Archive -Path $zip -DestinationPath $Tmp -Force
    $src = Join-Path $Tmp "apmux.exe"
    if (-not (Test-Path $src)) {
        throw "Binary 'apmux.exe' not found in archive."
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $staged = "$Target.new"
    Copy-Item -Path $src -Destination $staged -Force
    Move-Item -Path $staged -Destination $Target -Force
    Write-Info "Installed apmux.exe to $Target"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not $userPath) { $userPath = "" }
    $parts = $userPath -split ";" | Where-Object { $_ -ne "" }
    $onPath = $parts | Where-Object { $_.TrimEnd("\") -ieq $InstallDir.TrimEnd("\") }
    if (-not $onPath) {
        if ($SkipPath) {
            Write-Warn "$InstallDir is not in PATH (APMUX_SKIP_PATH=1; not modifying User PATH)"
        } else {
            $newPath = if ($userPath.Trim() -eq "") { $InstallDir } else { "$InstallDir;$userPath" }
            [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
            $env:Path = "$InstallDir;$env:Path"
            Write-Info "Added $InstallDir to the user PATH"
            Write-Host "  Open a new terminal, then run: apmux --version"
        }
    } else {
        $env:Path = "$InstallDir;$env:Path"
        Write-Host "  Run: apmux --version"
    }
} catch {
    Write-Err $_
    Write-Err "Manual download: $Releases"
    exit 1
} finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
