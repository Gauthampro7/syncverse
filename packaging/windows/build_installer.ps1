<#
.SYNOPSIS
    Builds the Syncverse Windows release binary and packages the Inno Setup installer.
.DESCRIPTION
    Requires:
      - Rust (cargo)
      - Inno Setup 6 (iscc.exe) in PATH or installed in standard Program Files
#>

[CmdletBinding()]
param(
    [switch]$SkipBuild = $false
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Resolve-Path "$PSScriptRoot\..\.."

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host " Syncverse Windows Installer Build Script" -ForegroundColor Cyan
Write-Host " Project root: $ProjectRoot" -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Compile Release Binary
if (-not $SkipBuild) {
    Write-Host "[1/3] Compiling Syncverse in Release mode..." -ForegroundColor Yellow
    Push-Location $ProjectRoot
    try {
        cargo build --release --bin syncverse
    }
    finally {
        Pop-Location
    }
} else {
    Write-Host "[1/3] Skipping Rust compilation as requested." -ForegroundColor DarkGray
}

$ExePath = "$ProjectRoot\target\release\syncverse.exe"
if (-not (Test-Path $ExePath)) {
    Write-Error "Release executable not found at: $ExePath. Ensure compilation succeeds."
    exit 1
}
Write-Host "  Found release binary: $ExePath" -ForegroundColor Green

# 2. Locate Inno Setup Compiler (ISCC)
Write-Host "[2/3] Locating Inno Setup 6 compiler (iscc)..." -ForegroundColor Yellow
$Iscc = Get-Command "iscc.exe" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
if (-not $Iscc) {
    $PotentialPaths = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "${env:ProgramFiles}\Inno Setup 6\ISCC.exe",
        "${env:LOCALAPPDATA}\Programs\Inno Setup 6\ISCC.exe"
    )
    foreach ($path in $PotentialPaths) {
        if (Test-Path $path) {
            $Iscc = $path
            break
        }
    }
}

if (-not $Iscc) {
    Write-Error "Inno Setup compiler (ISCC.exe) was not found in PATH or standard installation directories. Please install Inno Setup 6: winget install JRSoftware.InnoSetup or choco install innosetup"
    exit 1
}
Write-Host "  Found ISCC compiler: $Iscc" -ForegroundColor Green

# 3. Compile Installer Package
Write-Host "[3/3] Compiling Windows Installer with Inno Setup..." -ForegroundColor Yellow
$IssPath = "$PSScriptRoot\installer.iss"
$version = (Get-Content "$ProjectRoot\Cargo.toml" | Select-String '^version = "(.*)"' | ForEach-Object { $_.Matches.Groups[1].Value })
if (-not $version) { $version = "0.1.9" }
& $Iscc "/DMyAppVersion=$version" "$IssPath"
if ($LASTEXITCODE -ne 0) {
    Write-Error "ISCC compilation failed with exit code $LASTEXITCODE"
    exit $LASTEXITCODE
}

$DistDir = "$ProjectRoot\target\windows-dist"
Write-Host "==========================================================" -ForegroundColor Green
Write-Host " Build succeeded! Windows Installer generated at:" -ForegroundColor Green
Get-ChildItem -Path $DistDir -Filter "*.exe" | ForEach-Object {
    Write-Host "   -> $($_.FullName) ($([math]::Round($_.Length / 1MB, 2)) MB)" -ForegroundColor White
}
Write-Host "==========================================================" -ForegroundColor Green
