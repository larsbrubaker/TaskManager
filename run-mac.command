#!/bin/bash
# run-mac.command: double-click in Finder (or run from a terminal) to build and launch
# TaskManager on macOS. It builds an optimised release binary the first time, which takes a
# few minutes; later launches only rebuild what changed. Arguments are passed through to
# the app.
#
# The build expects the agg-gui, agg-rust and tess2-rust checkouts beside this repo
# (../agg-gui, ../agg-rust, ../tess2-rust), as in the rust-apps superproject; see README "Build from source".

set -u
cd "$(dirname "$0")" || exit 1

# Finder starts .command files in a fresh login shell, which may not have cargo on PATH.
if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
fi

fail() {
    echo
    echo "TaskManager: $1"
    exit 1
}

command -v cargo >/dev/null 2>&1 || fail "cargo was not found. Install Rust from https://rustup.rs and try again."
for sibling in agg-gui agg-rust tess2-rust; do
    [ -d "../$sibling" ] || fail "../$sibling was not found. Clone https://github.com/larsbrubaker/$sibling beside this folder (or use the rust-apps superproject)."
done

echo "Building TaskManager (release)..."
cargo build --release -p taskmanager-native || fail "the build failed (see the errors above)."

echo "Starting TaskManager..."
exec ./target/release/taskmanager "$@"
