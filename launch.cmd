@echo off
setlocal
cd /d "%~dp0"

if exist "dist\AxiomBrowser.exe" (
    "dist\AxiomBrowser.exe"
    if errorlevel 1 goto executable_failed
    exit /b 0
)

where cargo >nul 2>nul
if errorlevel 1 goto missing_cargo

echo AxiomBrowser.exe was not found in dist. Building and starting with Cargo...
cargo run --release
if errorlevel 1 goto run_failed
exit /b 0

:missing_cargo
echo ERROR: Cargo was not found. Install Rust and Cargo, then try again.
echo No files were installed or changed by this launcher.
pause
exit /b 1

:executable_failed
echo ERROR: AxiomBrowser.exe exited with an error.
pause
exit /b 1

:run_failed
echo ERROR: Cargo could not build or start Axiom Browser. Review the output above.
pause
exit /b 1
