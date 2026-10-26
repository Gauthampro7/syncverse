# Syncplay-Alt Windows Installer (PowerShell)
# Pure-Rust, zero-external-dependencies

Write-Host "==================================================" -ForegroundColor Cyan
Write-Host " Syncplay-Alt Windows Installer" -ForegroundColor Cyan
Write-Host "==================================================" -ForegroundColor Cyan

# 1. Build release binary
Write-Host "[1/3] Building optimized release binary..." -ForegroundColor Yellow
cargo build --release --workspace
if ($LASTEXITCODE -ne 0) {
    Write-Error "Build failed"
    exit 1
}

# 2. Determine target install directory
$InstallDir = "$env:LOCALAPPDATA\Programs\Syncplay-Alt"
if (!(Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

# 3. Copy executable
Write-Host "[2/3] Installing binary to $InstallDir\syncplay-alt.exe..." -ForegroundColor Yellow
Copy-Item "target\release\syncplay-alt.exe" "$InstallDir\syncplay-alt.exe" -Force

# 4. Check or Add to User PATH
Write-Host "[3/3] Checking User PATH environment variable..." -ForegroundColor Yellow
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    Write-Host "  Added $InstallDir to User PATH." -ForegroundColor Green
}

# 5. Create Desktop Shortcut
$WshShell = New-Object -ComObject WScript.Shell
$ShortcutPath = [System.IO.Path]::Combine([Environment]::GetFolderPath("Desktop"), "Syncplay-Alt.lnk")
$Shortcut = $WshShell.CreateShortcut($ShortcutPath)
$Shortcut.TargetPath = "$InstallDir\syncplay-alt.exe"
$Shortcut.Arguments = "play synthetic-mkv"
$Shortcut.Description = "Syncplay-Alt Standalone Media Player & Watch Party"
$Shortcut.WorkingDirectory = $InstallDir
$Shortcut.Save()

Write-Host "==================================================" -ForegroundColor Cyan
Write-Host " [SUCCESS] Syncplay-Alt successfully installed on Windows!" -ForegroundColor Green
Write-Host " Binary:    $InstallDir\syncplay-alt.exe"
Write-Host " Shortcut:  $ShortcutPath"
Write-Host " Launch:    syncplay-alt play synthetic-mkv (in Windows Terminal)"
Write-Host "==================================================" -ForegroundColor Cyan
