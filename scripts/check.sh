#!/bin/sh
# Run after every change: formatting, lints, and every test (the golden
# takes, the genome against the web app, the picture's invariants, the FFI).
set -e
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy -p syn-ffi --features cli --all-targets -- -D warnings
cargo test
