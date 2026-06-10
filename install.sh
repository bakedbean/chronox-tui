#!/usr/bin/env bash
#
# Build chronox in release mode and symlink it onto your PATH at
# ~/.local/bin/chronox, so you can run `chronox` from any directory.
#
# The symlink points at this repo's target/release build, so it stays valid
# across git pulls — just re-run `cargo build --release` (or this script) to
# pick up new changes.
#
# Usage:
#   ./install.sh              # install to ~/.local/bin
#   BIN_DIR=~/bin ./install.sh   # install elsewhere

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin_dir="${BIN_DIR:-$HOME/.local/bin}"
target="$repo_root/target/release/chronox"
link="$bin_dir/chronox"

echo "Building chronox (release)…"
cargo build --release --manifest-path "$repo_root/Cargo.toml"

mkdir -p "$bin_dir"
ln -sf "$target" "$link"
echo "Linked $link -> $target"

case ":$PATH:" in
  *":$bin_dir:"*) ;;
  *) echo "Note: $bin_dir is not on your PATH. Add it to your shell profile:"
     echo "  export PATH=\"$bin_dir:\$PATH\"" ;;
esac

echo "Done. Run 'chronox' or 'chronox /path/to/worktree'."
