@echo off
setlocal DisableDelayedExpansion
set "ERRORLEVEL="

pushd "%~dp0" || exit /b 1
cargo.exe run --quiet -- artisan %*
set "exit_code=%errorlevel%"
popd
exit /b %exit_code%
