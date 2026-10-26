#!/bin/sh
# installs the gargoyle cli, then run `gargoyle init` in your repo
#
#   curl -fsSL https://raw.githubusercontent.com/otarampinelli/gargoyle/main/install.sh | sh
#
# from a clone, `sh install.sh --local` installs the code in that checkout instead,
# your current branch and uncommitted changes included
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
  from_checkout=false
  for arg in "$@"; do
    case "$arg" in
      --local) from_checkout=true ;;
      *) fail "unknown option $arg, the only one is --local" ;;
    esac
  done

  has git || fail "gargoyle needs git, install it and run this again"
  has cargo || fail "gargoyle builds from source for now, install Rust from https://rustup.rs and run this again"

  if [ "$from_checkout" = true ]; then
    # the checkout this script sits in, piped through curl there's none
    root=$(cd "$(dirname "$0")" && pwd)
    [ -f "$root/crates/gargoyle-cli/Cargo.toml" ] ||
      fail "--local runs from a clone of gargoyle, like: sh ~/gargoyle/install.sh --local"
    say "installing the gargoyle cli from $root, it compiles and takes a minute or two"
    # --force replaces an installed gargoyle even when the version number is the same
    cargo install --locked --force --path "$root/crates/gargoyle-cli"
  else
    say "installing the gargoyle cli, it compiles from source and takes a minute or two"
    cargo install --locked --git "$REPO" gargoyle-cli
  fi

  say ""
  if ! has gargoyle; then
    say "gargoyle is in ~/.cargo/bin, add it to your PATH"
  fi
  say "done, now set up your repo:"
  say "  cd your-repo && gargoyle init"
}

main "$@"
