# ──────────────────────────────────────────────
#  Kyte 개발 환경 자동 설정 스크립트 (Windows)
# ──────────────────────────────────────────────
#Requires -RunAsAdministrator

$ErrorActionPreference = "Stop"
$LLVM_VERSION = "21"

function Write-Header {
    Write-Host ""
    Write-Host "  ██╗  ██╗██╗   ██╗████████╗███████╗" -ForegroundColor Cyan
    Write-Host "  ██║ ██╔╝╚██╗ ██╔╝╚══██╔══╝██╔════╝" -ForegroundColor Cyan
    Write-Host "  █████╔╝  ╚████╔╝    ██║   █████╗  " -ForegroundColor Cyan
    Write-Host "  ██╔═██╗   ╚██╔╝     ██║   ██╔══╝  " -ForegroundColor Cyan
    Write-Host "  ██║  ██╗   ██║      ██║   ███████╗" -ForegroundColor Cyan
    Write-Host "  ╚═╝  ╚═╝   ╚═╝      ╚═╝   ╚══════╝" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "  Development Environment Setup (Windows)" -ForegroundColor White
    Write-Host ""
}

function Info($msg)  { Write-Host "  [kyte] $msg" -ForegroundColor Cyan }
function Ok($msg)    { Write-Host "    ✓ $msg" -ForegroundColor Green }
function Err($msg)   { Write-Host "    ✗ $msg" -ForegroundColor Red }

Write-Header

# ── 1. Chocolatey ──
Info "Checking Chocolatey..."
if (Get-Command choco -ErrorAction SilentlyContinue) {
    Ok "Chocolatey found"
} else {
    Info "Installing Chocolatey..."
    Set-ExecutionPolicy Bypass -Scope Process -Force
    [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
    Invoke-Expression ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))
    Ok "Chocolatey installed"
}

# ── 2. Rust ──
Info "Checking Rust..."
if (Get-Command rustc -ErrorAction SilentlyContinue) {
    $rustVer = (rustc --version) -replace 'rustc ',''
    Ok "Rust $rustVer"
} else {
    Info "Installing Rust..."
    choco install rustup.install -y
    refreshenv
    rustup default stable
    Ok "Rust installed"
}
rustup component add rustfmt clippy 2>$null
Ok "rustfmt + clippy"

# ── 3. LLVM ──
Info "Checking LLVM $LLVM_VERSION..."
$llvmPath = "C:\Program Files\LLVM"
if (Test-Path "$llvmPath\bin\llvm-config.exe") {
    Ok "LLVM found at $llvmPath"
} else {
    Info "Installing LLVM $LLVM_VERSION via Chocolatey..."
    choco install llvm --version="${LLVM_VERSION}.0.0" -y
    Ok "LLVM $LLVM_VERSION installed"
}

# 환경 변수 설정
$env:LLVM_SYS_211_PREFIX = $llvmPath
[System.Environment]::SetEnvironmentVariable("LLVM_SYS_211_PREFIX", $llvmPath, "User")
Ok "LLVM_SYS_211_PREFIX = $llvmPath"

# ── 4. Clang ──
Info "Checking Clang..."
if (Get-Command clang -ErrorAction SilentlyContinue) {
    Ok "Clang found"
} else {
    Err "Clang not found. LLVM installation should include clang."
    Err "Ensure '$llvmPath\bin' is in your PATH."
}

# ── 5. Node.js ──
Info "Checking Node.js..."
if (Get-Command node -ErrorAction SilentlyContinue) {
    Ok "Node $(node --version)"
} else {
    Info "Installing Node.js..."
    choco install nodejs-lts -y
    refreshenv
    Ok "Node.js installed"
}

# ── 6. 빌드 ──
Info "Building Kyte..."
cargo build --release
if ($LASTEXITCODE -eq 0) {
    Ok "Build complete: target\release\kyte.exe"
} else {
    Err "Build failed! Check LLVM installation."
    exit 1
}

# ── 7. VS Code 확장 ──
if (Test-Path "editors\vscode\package.json") {
    Info "Installing VS Code extension dependencies..."
    Push-Location editors\vscode
    npm install
    Pop-Location
    Ok "VS Code extension ready"
}

# ── 8. PATH에 추가 ──
$kyteBin = (Resolve-Path "target\release").Path
$currentPath = [System.Environment]::GetEnvironmentVariable("PATH", "User")
if ($currentPath -notlike "*$kyteBin*") {
    Info "Adding kyte to user PATH..."
    [System.Environment]::SetEnvironmentVariable("PATH", "$kyteBin;$currentPath", "User")
    $env:PATH = "$kyteBin;$env:PATH"
    Ok "Added $kyteBin to PATH"
} else {
    Ok "kyte already in PATH"
}

# ── 완료 ──
Write-Host ""
Write-Host "  Setup complete!" -ForegroundColor Green -BackgroundColor Black
Write-Host ""
Write-Host "  Quick start:" -ForegroundColor White
Write-Host "    kyte examples\hello.ky        # Compile" -ForegroundColor Gray
Write-Host "    kyte lsp                       # LSP server" -ForegroundColor Gray
Write-Host "    kyte test                      # Test suite" -ForegroundColor Gray
Write-Host ""
