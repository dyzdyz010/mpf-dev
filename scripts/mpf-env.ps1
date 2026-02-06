# MPF Development Environment Setup (Windows PowerShell)
# Usage: . .\mpf-env.ps1 [qt_path]
#
# Sets up environment variables for local MPF development:
# - CMAKE_PREFIX_PATH: Qt + SDK paths
# - QML_IMPORT_PATH: QML module paths
# - PATH: MinGW bin path

param(
    [string]$QtPath = "C:\Qt\6.8.3\mingw_64"
)

# Detect SDK location
$SdkRoot = "$env:USERPROFILE\.mpf-sdk"
$CurrentSdk = Get-Content "$SdkRoot\current.txt" -ErrorAction SilentlyContinue
if (-not $CurrentSdk) {
    Write-Host "Error: No SDK installed. Run 'mpf-dev setup' first." -ForegroundColor Red
    return
}
$SdkPath = "$SdkRoot\$CurrentSdk"

# Set CMAKE_PREFIX_PATH
$env:CMAKE_PREFIX_PATH = "$QtPath;$SdkPath"
$env:MPF_CMAKE_PREFIX_PATH = $env:CMAKE_PREFIX_PATH

# Set QML_IMPORT_PATH
$QmlPaths = @(
    "$SdkPath\qml",
    "$QtPath\qml"
)
$env:QML_IMPORT_PATH = $QmlPaths -join ";"
$env:MPF_QML_IMPORT_PATH = $env:QML_IMPORT_PATH

# Add MinGW to PATH if Qt path contains mingw
if ($QtPath -match "mingw") {
    $MingwBin = "$QtPath\..\..\Tools\mingw1310_64\bin"
    if (Test-Path $MingwBin) {
        $env:PATH = "$MingwBin;$env:PATH"
        Write-Host "Added MinGW to PATH: $MingwBin" -ForegroundColor Green
    }
}

# Display summary
Write-Host ""
Write-Host "MPF Development Environment" -ForegroundColor Cyan
Write-Host "============================" -ForegroundColor Cyan
Write-Host "Qt Path:           $QtPath"
Write-Host "SDK Path:          $SdkPath"
Write-Host "CMAKE_PREFIX_PATH: $env:CMAKE_PREFIX_PATH"
Write-Host "QML_IMPORT_PATH:   $env:QML_IMPORT_PATH"
Write-Host ""
Write-Host "Ready! You can now configure projects with:" -ForegroundColor Green
Write-Host "  cmake -B build -G 'MinGW Makefiles'" -ForegroundColor Yellow
Write-Host ""
Write-Host "Or use CMake presets (if CMakeUserPresets.json exists):" -ForegroundColor Green
Write-Host "  cmake --preset dev" -ForegroundColor Yellow
