@echo off
REM Cargo runner for Windows: put GStreamer DLLs on PATH before starting the binary.
set "GST_ROOT=%GSTREAMER_1_0_ROOT_MSVC_X86_64%"
if "%GST_ROOT%"=="" set "GST_ROOT=C:\Program Files\gstreamer\1.0\msvc_x86_64"
if exist "%GST_ROOT%\bin\" (
  set "PATH=%GST_ROOT%\bin;%PATH%"
  set "GST_PLUGIN_PATH=%GST_ROOT%\lib\gstreamer-1.0"
  set "GSTREAMER_1_0_ROOT_MSVC_X86_64=%GST_ROOT%"
)
if exist "C:\ProgramData\chocolatey\lib\ffmpeg\tools\ffmpeg\bin\" (
  set "PATH=C:\ProgramData\chocolatey\lib\ffmpeg\tools\ffmpeg\bin;%PATH%"
)
%*
exit /b %ERRORLEVEL%
