#!/usr/bin/env bash
# Install catmd. Works two ways:
#   ./scripts/install.sh                  # from a local checkout
#   curl -fsSL https://raw.githubusercontent.com/CristianosLeite/catmd/main/scripts/install.sh | bash
# Either way it runs scripts/build-linux.sh --install, which installs build
# prerequisites, builds the release binary, and installs it to ~/.cargo/bin.
set -euo pipefail

REPO="CristianosLeite/catmd"
BRANCH="main"

# Local checkout: the script sits next to build-linux.sh.
if [ -n "${BASH_SOURCE[0]:-}" ] && [ -f "${BASH_SOURCE[0]}" ]; then
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    if [ -x "$script_dir/build-linux.sh" ]; then
        exec "$script_dir/build-linux.sh" --install
    fi
fi

# Standalone (curl | bash): download the source first, then build + install.
for tool in curl tar; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "error: '$tool' is required to download catmd" >&2
        exit 1
    fi
done

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

echo "==> Downloading $REPO ($BRANCH)"
curl -fsSL "https://github.com/$REPO/archive/refs/heads/$BRANCH.tar.gz" |
    tar -xz -C "$tmpdir" --strip-components=1

"$tmpdir/scripts/build-linux.sh" --install
