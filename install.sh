#!/bin/sh
# installs the gargoyle cli, then run `gargoyle init` in your repo
#
#   curl -fsSL https://raw.githubusercontent.com/otarampinelli/gargoyle/main/install.sh | sh
#
# read it before you run it, it's short on purpose and never uses sudo

set -eu

REPO="https://github.com/otarampinelli/gargoyle"

say() { printf '%s\n' "$*"; }
fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}
has() { command -v "$1" >/dev/null 2>&1; }

# everything runs from here, so a download cut in half runs nothing
main() {
  has git || fail "gargoyle needs git, install it and run this again"
  has cargo || fail "gargoyle builds from source for now, install Rust from https://rustup.rs and run this again"

  say "installing the gargoyle cli, it compiles from source and takes a minute or two"
  cargo install --locked --git "$REPO" gargoyle-cli

  say ""
  if ! has gargoyle; then
    say "gargoyle is in ~/.cargo/bin, add it to your PATH"
  fi
  say "done, now set up your repo:"
  say "  cd your-repo && gargoyle init"
}

main "$@"
