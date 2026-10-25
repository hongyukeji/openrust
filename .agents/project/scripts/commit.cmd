@echo off
rem commit.cmd — Windows 入口，自包含，只配套同目录 commit 脚本，不依赖其他文件。
rem 需要 Git for Windows Bash：加入 PATH，或设置 BASH_EXE=C:\path\to\bash.exe。
setlocal

set "TARGET_SCRIPT=%~dp0commit"
if not exist "%TARGET_SCRIPT%" (
  echo commit script not found: %TARGET_SCRIPT% 1>&2
  exit /b 1
)

rem 转成 Bash 路径：反斜杠转正斜杠，盘符 C: 转 /c。
set "BASH_TARGET=%TARGET_SCRIPT:\=/%"
if not "%BASH_TARGET:~1,1%"==":" goto after_drive
set "BASH_DRIVE=%BASH_TARGET:~0,1%"
for %%D in (a b c d e f g h i j k l m n o p q r s t u v w x y z) do (
  if /I "%BASH_DRIVE%"=="%%D" set "BASH_DRIVE=%%D"
)
set "BASH_TARGET=/%BASH_DRIVE%%BASH_TARGET:~2%"
:after_drive

set "BASH_CMD="
if defined BASH_EXE (
  if exist "%BASH_EXE%" (
    set "BASH_CMD=%BASH_EXE%"
  ) else (
    for /f "delims=" %%I in ('where "%BASH_EXE%" 2^>nul') do if not defined BASH_CMD set "BASH_CMD=%%I"
  )
)
if not defined BASH_CMD if exist "%ProgramFiles%\Git\bin\bash.exe" set "BASH_CMD=%ProgramFiles%\Git\bin\bash.exe"
if not defined BASH_CMD if exist "%ProgramFiles(x86)%\Git\bin\bash.exe" set "BASH_CMD=%ProgramFiles(x86)%\Git\bin\bash.exe"
if not defined BASH_CMD if exist "%LocalAppData%\Programs\Git\bin\bash.exe" set "BASH_CMD=%LocalAppData%\Programs\Git\bin\bash.exe"
if not defined BASH_CMD for /f "delims=" %%I in ('where bash.exe 2^>nul') do if not defined BASH_CMD set "BASH_CMD=%%I"
if not defined BASH_CMD for /f "delims=" %%I in ('where bash 2^>nul') do if not defined BASH_CMD set "BASH_CMD=%%I"

if not defined BASH_CMD (
  echo Git for Windows Bash is required to run commit.cmd. 1>&2
  echo Install Git for Windows, add bash.exe to PATH, or set BASH_EXE=C:\path\to\bash.exe. 1>&2
  exit /b 1
)

"%BASH_CMD%" "%BASH_TARGET%" %*
exit /b %ERRORLEVEL%
