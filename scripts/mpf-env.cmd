@echo off
REM MPF Development Environment Setup (Windows CMD)
REM Usage: mpf-env.cmd [qt_path]
REM
REM Sets CMAKE_PREFIX_PATH and QML_IMPORT_PATH for local development

setlocal enabledelayedexpansion

REM Default Qt path
set "QT_PATH=%~1"
if "%QT_PATH%"=="" set "QT_PATH=C:\Qt\6.8.3\mingw_64"

REM Find SDK
set "SDK_ROOT=%USERPROFILE%\.mpf-sdk"
if not exist "%SDK_ROOT%\current.txt" (
    echo Error: No SDK installed. Run 'mpf-dev setup' first.
    exit /b 1
)
set /p CURRENT_SDK=<"%SDK_ROOT%\current.txt"
set "SDK_PATH=%SDK_ROOT%\%CURRENT_SDK%"

REM Set environment variables (persistent for this session)
endlocal & (
    set "CMAKE_PREFIX_PATH=%QT_PATH%;%SDK_PATH%"
    set "MPF_CMAKE_PREFIX_PATH=%QT_PATH%;%SDK_PATH%"
    set "QML_IMPORT_PATH=%SDK_PATH%\qml;%QT_PATH%\qml"
    set "MPF_QML_IMPORT_PATH=%SDK_PATH%\qml;%QT_PATH%\qml"
)

echo.
echo MPF Development Environment
echo ============================
echo Qt Path:           %QT_PATH%
echo SDK Path:          %SDK_PATH%
echo CMAKE_PREFIX_PATH: %CMAKE_PREFIX_PATH%
echo QML_IMPORT_PATH:   %QML_IMPORT_PATH%
echo.
echo Ready! Configure projects with:
echo   cmake -B build -G "MinGW Makefiles"
echo.
