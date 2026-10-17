@echo off
rem run-windows.cmd: double-click in Explorer (or run from a terminal) to build and launch
rem TaskManager on Windows. It builds an optimised release binary the first time, which takes
rem a few minutes; later launches only rebuild what changed. Arguments are passed through to
rem the app.
rem The Windows counterpart of run-mac.command.
rem
rem The build expects the agg-gui, agg-rust and tess2-rust checkouts beside this repo
rem (..\agg-gui, ..\agg-rust, ..\tess2-rust), as in the rust-apps superproject; see README "Build from source".

setlocal
cd /d "%~dp0" || exit /b 1

rem A shell opened before rustup was installed may not have cargo on PATH yet.
where cargo >nul 2>nul
if errorlevel 1 if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

where cargo >nul 2>nul || goto no_cargo
set "TASKMANAGER_SIBLING=agg-gui"
if not exist "..\agg-gui\" goto no_sibling
set "TASKMANAGER_SIBLING=agg-rust"
if not exist "..\agg-rust\" goto no_sibling
set "TASKMANAGER_SIBLING=tess2-rust"
if not exist "..\tess2-rust\" goto no_sibling

echo Building TaskManager (release)...
cargo build --release -p taskmanager-native || goto build_failed

echo Starting TaskManager...
"target\release\taskmanager.exe" %*
exit /b %errorlevel%

:no_cargo
set "TASKMANAGER_FAIL=cargo was not found. Install Rust from https://rustup.rs and try again."
goto fail

:no_sibling
set "TASKMANAGER_FAIL=..\%TASKMANAGER_SIBLING% was not found. Clone https://github.com/larsbrubaker/%TASKMANAGER_SIBLING% beside this folder (or use the rust-apps superproject)."
goto fail

:build_failed
set "TASKMANAGER_FAIL=the build failed (see the errors above). If it asks for a newer rustc, run: rustup update"
goto fail

:fail
echo.
echo TaskManager: %TASKMANAGER_FAIL%
rem Keep the console window open so the message can be read after a double-click.
pause
exit /b 1
