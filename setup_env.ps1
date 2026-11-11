# =========================================================================
# ESP32-C3 RUST (ESP-IDF) ENVIRONMENT SETUP SCRIPT
# Run this script once on a new PC/laptop via PowerShell:
# .\setup_env.ps1
# =========================================================================

Write-Host "=========================================================" -ForegroundColor Cyan
Write-Host "   ESP32-C3 RUST ENVIRONMENT SETUP (OPEN SOLAR GATEWAY)  " -ForegroundColor Cyan
Write-Host "=========================================================" -ForegroundColor Cyan

# 1. Check Rustup
Write-Host "`n[1/6] Checking Rust & Cargo Installation..." -ForegroundColor Yellow
if (-not (Get-Command "rustup" -ErrorAction SilentlyContinue)) {
    Write-Host "❌ Rustup is not installed! Download and install from https://rustup.rs/" -ForegroundColor Red
    exit 1
}
Write-Host "✅ Rust detected: $(rustc --version)" -ForegroundColor Green

# 2. Add RISC-V Target for ESP32-C3
Write-Host "`n[2/6] Installing target riscv32imc-esp-espidf..." -ForegroundColor Yellow
rustup target add riscv32imc-esp-espidf
Write-Host "✅ Target riscv32imc-esp-espidf installed." -ForegroundColor Green

# 3. Check / Install ldproxy
Write-Host "`n[3/6] Checking ldproxy (Espressif Linker Wrapper)..." -ForegroundColor Yellow
if (-not (Get-Command "ldproxy" -ErrorAction SilentlyContinue)) {
    Write-Host "⏳ Installing ldproxy via cargo install..." -ForegroundColor DarkYellow
    cargo install ldproxy
} else {
    Write-Host "✅ ldproxy is already installed." -ForegroundColor Green
}

# 4. Check / Install espflash
Write-Host "`n[4/6] Checking espflash (Serial Flashing Tool)..." -ForegroundColor Yellow
if (-not (Get-Command "espflash" -ErrorAction SilentlyContinue)) {
    Write-Host "⏳ Installing espflash via cargo install..." -ForegroundColor DarkYellow
    cargo install espflash --locked
} else {
    Write-Host "✅ espflash is already installed: $(espflash --version)" -ForegroundColor Green
}

# 5. Check Prerequisites (Git, Python, CMake, Ninja)
Write-Host "`n[5/6] Checking Build Tools (Git, Python, CMake, Ninja)..." -ForegroundColor Yellow
$missing = @()
foreach ($cmd in @("git", "python", "cmake", "ninja")) {
    if (Get-Command $cmd -ErrorAction SilentlyContinue) {
        Write-Host "  ✅ $cmd installed." -ForegroundColor Green
    } else {
        Write-Host "  ⚠️ $cmd not detected in PATH!" -ForegroundColor Yellow
        $missing += $cmd
    }
}

if ($missing.Count -gt 0) {
    Write-Host "`nTip: You can install missing build tools using winget:" -ForegroundColor Cyan
    Write-Host "winget install Git.Git Kitware.CMake Ninja-build.Ninja Python.Python.3.11" -ForegroundColor White
}

# 6. Prepare Short Target Build Directory (Prevents Windows Long Path UNC Errors)
Write-Host "`n[6/6] Preparing short target build folder C:\esp-target..." -ForegroundColor Yellow
if (-not (Test-Path "C:\esp-target")) {
    New-Item -ItemType Directory -Path "C:\esp-target" -Force | Out-Null
    Write-Host "✅ Folder C:\esp-target created." -ForegroundColor Green
} else {
    Write-Host "✅ Folder C:\esp-target is ready." -ForegroundColor Green
}

# Copy partitions.csv to C:\esp-target for build consistency
if (Test-Path "partitions.csv") {
    Copy-Item "partitions.csv" "C:\esp-target\partitions.csv" -Force
    Write-Host "✅ partitions.csv copied to C:\esp-target." -ForegroundColor Green
}

Write-Host "`n=========================================================" -ForegroundColor Green
Write-Host "  🎉 SETUP COMPLETE! ENVIRONMENT IS READY." -ForegroundColor Green
Write-Host "  To build and flash the firmware:" -ForegroundColor White
Write-Host "  cargo build" -ForegroundColor Cyan
Write-Host "  espflash flash --port COMx --partition-table partitions.csv C:\esp-target\riscv32imc-esp-espidf\debug\open-solar-gateway" -ForegroundColor Cyan
Write-Host "=========================================================" -ForegroundColor Green

