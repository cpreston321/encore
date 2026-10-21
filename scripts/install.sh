#!/bin/bash
# Builds Encore from this checkout and installs it to ~/Applications, setting up what the
# build needs first: the Xcode Command Line Tools and Rust.
#
#   git clone https://github.com/cpreston321/encore.git && cd encore && ./scripts/install.sh
#
# An app built on your Mac isn't quarantined, so Gatekeeper doesn't ask about it, and it's
# signed with your own certificate when you have one (scripts/sign.sh).
set -euo pipefail
cd "$(dirname "$0")/.."

min_rust=1.88

if [ "$(uname)" != Darwin ]; then
  echo "Encore is a Mac app; it doesn't build on $(uname)." >&2
  exit 1
fi
if [ "$(sw_vers -productVersion | cut -d. -f1)" -lt 13 ]; then
  echo "Encore needs macOS 13 or later." >&2
  exit 1
fi

if ! xcode-select -p >/dev/null 2>&1; then
  echo "Encore needs the Xcode Command Line Tools to build. Opening their installer…"
  xcode-select --install || true
  echo "Run ./scripts/install.sh again once they've finished installing."
  exit 1
fi

# rustup installs into ~/.cargo/bin, which a new shell may not have on its PATH yet.
[ -x "$HOME/.cargo/bin/cargo" ] && export PATH="$HOME/.cargo/bin:$PATH"

if ! command -v cargo >/dev/null 2>&1; then
  if [ ! -t 0 ]; then
    echo "Encore needs Rust $min_rust or later; install it from https://rustup.rs and run this again." >&2
    exit 1
  fi
  read -r -p "Encore needs Rust to build. Install it with rustup (https://rustup.rs)? [y/N] " answer
  case "$answer" in
    [Yy]*) ;;
    *) echo "Install Rust $min_rust or later, then run this again."; exit 1 ;;
  esac
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  export PATH="$HOME/.cargo/bin:$PATH"
fi

rust=$(rustc --version | cut -d' ' -f2)
if [ "$(printf '%s\n%s\n' "$min_rust" "$rust" | sort -V | head -1)" != "$min_rust" ]; then
  if command -v rustup >/dev/null 2>&1; then
    echo "Updating Rust $rust to the latest stable release…"
    rustup update stable
  else
    echo "Encore needs Rust $min_rust or later, and this is $rust." >&2
    exit 1
  fi
fi

echo "Building Encore. The first build takes a few minutes…"
DIST=1 ./scripts/build.sh

cat <<'DONE'

Encore is in ~/Applications. Open it with:

  open ~/Applications/Encore.app

To sign in to Apple Music and play full songs, Encore needs a MusicKit key from an Apple
Developer account; the app walks you through adding one (more in docs/musickit.md).
Without one you can browse and play 30-second previews.

To update later: git pull && ./scripts/install.sh
DONE
