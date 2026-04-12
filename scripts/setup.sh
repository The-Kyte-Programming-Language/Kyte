#!/usr/bin/env bash
# ──────────────────────────────────────────────
#  Kyte 개발 환경 자동 설정 스크립트 (Linux / macOS)
# ──────────────────────────────────────────────
set -euo pipefail

LLVM_VERSION=21
BOLD="\033[1m"
GREEN="\033[1;32m"
CYAN="\033[1;36m"
RED="\033[1;31m"
RESET="\033[0m"

info()  { echo -e "${CYAN}[kyte]${RESET} $1"; }
ok()    { echo -e "${GREEN}  ✓${RESET} $1"; }
err()   { echo -e "${RED}  ✗${RESET} $1"; }

echo -e "${BOLD}"
echo "  ██╗  ██╗██╗   ██╗████████╗███████╗"
echo "  ██║ ██╔╝╚██╗ ██╔╝╚══██╔══╝██╔════╝"
echo "  █████╔╝  ╚████╔╝    ██║   █████╗  "
echo "  ██╔═██╗   ╚██╔╝     ██║   ██╔══╝  "
echo "  ██║  ██╗   ██║      ██║   ███████╗"
echo "  ╚═╝  ╚═╝   ╚═╝      ╚═╝   ╚══════╝"
echo -e "${RESET}"
echo "  Development Environment Setup"
echo ""

# ── 1. Rust ──
info "Checking Rust toolchain..."
if command -v rustc &>/dev/null; then
    ok "Rust $(rustc --version | awk '{print $2}')"
else
    info "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    ok "Rust installed"
fi

# rustfmt & clippy
rustup component add rustfmt clippy 2>/dev/null
ok "rustfmt + clippy"

# ── 2. LLVM ──
info "Checking LLVM ${LLVM_VERSION}..."
OS="$(uname -s)"
if command -v llvm-config-${LLVM_VERSION} &>/dev/null; then
    ok "LLVM ${LLVM_VERSION} already installed"
elif command -v llvm-config &>/dev/null; then
    FOUND=$(llvm-config --version | cut -d. -f1)
    if [ "$FOUND" = "$LLVM_VERSION" ]; then
        ok "LLVM ${LLVM_VERSION} found"
    else
        err "LLVM $FOUND found, but ${LLVM_VERSION} required"
        exit 1
    fi
else
    info "Installing LLVM ${LLVM_VERSION}..."
    if [ "$OS" = "Linux" ]; then
        wget -qO- https://apt.llvm.org/llvm-snapshot.gpg.key | sudo apt-key add -
        CODENAME=$(lsb_release -cs)
        echo "deb http://apt.llvm.org/${CODENAME}/ llvm-toolchain-${CODENAME}-${LLVM_VERSION} main" \
            | sudo tee /etc/apt/sources.list.d/llvm.list
        sudo apt-get update -qq
        sudo apt-get install -y -qq llvm-${LLVM_VERSION}-dev libpolly-${LLVM_VERSION}-dev clang-${LLVM_VERSION}
        ok "LLVM ${LLVM_VERSION} installed (apt)"
    elif [ "$OS" = "Darwin" ]; then
        brew install llvm@${LLVM_VERSION}
        ok "LLVM ${LLVM_VERSION} installed (brew)"
    else
        err "Unsupported OS: $OS"
        exit 1
    fi
fi

# LLVM_SYS prefix
if [ "$OS" = "Linux" ]; then
    export LLVM_SYS_211_PREFIX="/usr/lib/llvm-${LLVM_VERSION}"
elif [ "$OS" = "Darwin" ]; then
    export LLVM_SYS_211_PREFIX="$(brew --prefix llvm@${LLVM_VERSION})"
fi
ok "LLVM_SYS_211_PREFIX=$LLVM_SYS_211_PREFIX"

# ── 3. Clang (linker) ──
info "Checking Clang..."
if command -v clang &>/dev/null; then
    ok "Clang $(clang --version | head -1)"
else
    err "Clang not found. Install clang to link Kyte programs."
    exit 1
fi

# ── 4. Node.js (VS Code 확장) ──
info "Checking Node.js..."
if command -v node &>/dev/null; then
    ok "Node $(node --version)"
else
    info "Node.js not found. Skipping VS Code extension setup."
fi

# ── 5. 빌드 ──
info "Building Kyte..."
cargo build --release
ok "Build complete: target/release/kyte"

# ── 6. VS Code 확장 ──
if command -v node &>/dev/null && [ -d "editors/vscode" ]; then
    info "Installing VS Code extension dependencies..."
    cd editors/vscode
    npm install
    cd ../..
    ok "VS Code extension ready"
fi

# ── 완료 ──
echo ""
echo -e "${GREEN}${BOLD}  Setup complete!${RESET}"
echo ""
echo "  Quick start:"
echo "    cargo run --release -- examples/hello.ky     # Compile"
echo "    cargo run --release -- lsp                    # LSP server"
echo "    cargo run --release -- test                   # Test suite"
echo ""
echo "  Environment variables (add to your shell profile):"
echo "    export LLVM_SYS_211_PREFIX=\"$LLVM_SYS_211_PREFIX\""
echo ""
