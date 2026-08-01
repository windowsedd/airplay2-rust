@echo off
REM Cargo "runner" for Windows: put GStreamer DLLs on PATH before starting the binary.
REM Without this, airplay-app.exe exits with 0xC0000135 STATUS_DLL_NOT_FOUND.

set "GST_ROOT=%GSTREAMER_1_0_ROOT_MSVC_X86_64%"
if "%GST_ROOT%"=="" set "GST_ROOT=C:\Program Files\gstreamer\1.0\msvc_x86_64"

if exist "%GST_ROOT%\bin\gstreamer-1.0-0.dll" (
  set "PATH=%GST_ROOT%\bin;%PATH%"
  set "GST_PLUGIN_PATH=%GST_ROOT%\lib\gstreamer-1.0"
  set "GSTREAMER_1_0_ROOT_MSVC_X86_64=%GST_ROOT%"
) else if exist "%GST_ROOT%\bin\" (
  set "PATH=%GST_ROOT%\bin;%PATH%"
  set "GST_PLUGIN_PATH=%GST_ROOT%\lib\gstreamer-1.0"
  set "GSTREAMER_1_0_ROOT_MSVC_X86_64=%GST_ROOT%"
)

REM Real ffplay (optional)
if exist "C:\ProgramData\chocolatey\lib\ffmpeg\tools\ffmpeg\bin\" (
  set "PATH=C:\ProgramData\chocolatey\lib\ffmpeg\tools\ffmpeg\bin;%PATH%"
)
if exist "C:\ProgramData\chocolatey\bin\" (
  set "PATH=C:\ProgramData\chocolatey\bin;%PATH%"
)

REM Run the program (all args: path\to\exe.exe [args...])
%*
exit /b %ERRORLEVEL%
