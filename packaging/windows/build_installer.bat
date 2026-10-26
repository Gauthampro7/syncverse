@echo off
rem ==============================================================================
rem Syncplay-Alt Windows Installer Build Batch Script
rem ==============================================================================

setlocal enabledelayedexpansion
pushd "%~dp0..\.."
set "PROJECT_ROOT=%CD%"

echo ==========================================================
echo  Syncplay-Alt Windows Installer Builder
echo ==========================================================

echo [1/3] Compiling syncplay-alt in release mode...
cargo build --release --bin syncplay-alt
if errorlevel 1 (
    echo [ERROR] Cargo release build failed.
    popd
    exit /b 1
)

echo [2/3] Checking Inno Setup Compiler...
set "ISCC="
where iscc.exe >nul 2>nul
if %errorlevel% equ 0 (
    set "ISCC=iscc.exe"
) else if exist "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" (
    set "ISCC=%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe"
) else if exist "%ProgramFiles%\Inno Setup 6\ISCC.exe" (
    set "ISCC=%ProgramFiles%\Inno Setup 6\ISCC.exe"
) else if exist "%LocalAppData%\Programs\Inno Setup 6\ISCC.exe" (
    set "ISCC=%LocalAppData%\Programs\Inno Setup 6\ISCC.exe"
)

if "%ISCC%"=="" (
    echo [ERROR] Inno Setup 6 (ISCC.exe) not found.
    echo Please install Inno Setup 6: winget install JRSoftware.InnoSetup
    popd
    exit /b 1
)

echo [3/3] Building Installer with "%ISCC%"...
"%ISCC%" "%PROJECT_ROOT%\packaging\windows\installer.iss"
if errorlevel 1 (
    echo [ERROR] Inno Setup compilation failed.
    popd
    exit /b 1
)

echo ==========================================================
echo  Installer build complete!
echo  Installer is in: %PROJECT_ROOT%\target\windows-dist\
echo ==========================================================
popd
exit /b 0
