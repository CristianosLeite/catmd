#!/usr/bin/env bash
# Build catmd on any common Linux distro: install build prerequisites with the
# native package manager, bootstrap Rust via rustup if needed, then build the
# release binary. Usage: ./scripts/build-linux.sh [--install]
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

DO_INSTALL=0
for arg in "$@"; do
    case "$arg" in
        --install) DO_INSTALL=1 ;;
        -h|--help)
            echo "Usage: $0 [--install]"
            echo ""
            echo "Installs build prerequisites (C compiler, pkg-config, curl) using the"
            echo "distro's package manager, installs Rust via rustup if missing, and runs"
            echo "'cargo build --release'."
            echo ""
            echo "  --install   also run 'cargo install --path .' (installs to ~/.cargo/bin)"
            exit 0
            ;;
        *)
            echo "error: unknown option '$arg' (try --help)" >&2
            exit 2
            ;;
    esac
done

if [ "$(uname -s)" != "Linux" ]; then
    echo "error: catmd is Linux-only and this script targets Linux distros." >&2
    exit 1
fi

# Run a package-manager command, with sudo when not root.
run_priv() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif command -v sudo >/dev/null 2>&1; then
        sudo "$@"
    else
        echo "error: need root or sudo to run: $*" >&2
        exit 1
    fi
}

have_prereqs() {
    { command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1 || command -v clang >/dev/null 2>&1; } &&
        command -v pkg-config >/dev/null 2>&1 &&
        command -v curl >/dev/null 2>&1
}

install_prereqs() {
    # Identify the distro family via os-release.
    local id="" id_like=""
    if [ -r /etc/os-release ]; then
        # shellcheck disable=SC1091
        . /etc/os-release
        id="${ID:-}"
        id_like="${ID_LIKE:-}"
    fi

    case " $id $id_like " in
        *" debian "*|*" ubuntu "*|*" linuxmint "*|*" pop "*)
            echo "==> Installing prerequisites with apt-get (Debian/Ubuntu family)"
            run_priv apt-get update
            run_priv apt-get install -y build-essential pkg-config curl
            ;;
        *" fedora "*|*" rhel "*|*" centos "*|*" rocky "*|*" almalinux "*)
            echo "==> Installing prerequisites with dnf/yum (Fedora/RHEL family)"
            if command -v dnf >/dev/null 2>&1; then
                run_priv dnf install -y gcc pkg-config curl
            else
                run_priv yum install -y gcc pkg-config curl
            fi
            ;;
        *" arch "*|*" manjaro "*|*" endeavouros "*)
            echo "==> Installing prerequisites with pacman (Arch family)"
            # No -Sy: a database-only refresh risks a partial upgrade. If this
            # fails because the local database is stale, run 'pacman -Syu' first.
            run_priv pacman -S --needed --noconfirm base-devel pkg-config curl
            ;;
        *" opensuse"*|*" suse "*|*" sles "*)
            echo "==> Installing prerequisites with zypper (openSUSE/SUSE family)"
            run_priv zypper --non-interactive install gcc pkg-config curl
            ;;
        *" alpine "*)
            echo "==> Installing prerequisites with apk (Alpine)"
            run_priv apk add build-base pkgconf curl
            ;;
        *)
            echo "warning: unrecognized distro '${id:-unknown}'." >&2
            echo "Install these manually, then re-run: a C compiler (gcc/clang), pkg-config, curl." >&2
            exit 1
            ;;
    esac
}

if have_prereqs; then
    echo "==> Build prerequisites already present, skipping package installation"
else
    install_prereqs
fi

# Ensure a Rust toolchain.
if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
    # rustup is installed but not on PATH in this shell.
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "==> Rust not found, installing via rustup"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
fi

# Warn if the toolchain is older than Cargo.toml's rust-version.
min_rust="$(sed -n 's/^rust-version *= *"\(.*\)"/\1/p' "$REPO_ROOT/Cargo.toml")"
rustc_ver="$(rustc --version | awk '{print $2}')"
if [ -n "$min_rust" ] && [ "$(printf '%s\n' "$min_rust" "$rustc_ver" | sort -V | head -n1)" != "$min_rust" ]; then
    echo "warning: rustc $rustc_ver is older than the required $min_rust." >&2
    echo "Run 'rustup update' (or update your distro's Rust) and try again." >&2
    exit 1
fi

if [ "$DO_INSTALL" -eq 1 ]; then
    echo "==> Building and installing to ~/.cargo/bin (rustc $rustc_ver)"
    cargo install --locked --path "$REPO_ROOT"
else
    echo "==> Building release binary (rustc $rustc_ver)"
    cargo build --release --locked --manifest-path "$REPO_ROOT/Cargo.toml"
    echo ""
    echo "Build complete: $REPO_ROOT/target/release/catmd"
    echo "Tip: '$0 --install' (or 'cargo install --locked --path .') installs it to ~/.cargo/bin."
fi
