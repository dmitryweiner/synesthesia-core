#!/bin/sh
# Run after every change: formatting, lints, and every test (the golden
# takes, the genome against the web app, the picture's invariants, the FFI).
set -e
cd "$(dirname "$0")/.."
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo clippy -p syn-ffi --features cli --all-targets -- -D warnings
cargo test
# syn-wasm as the web app loads it: built for wasm32 and tested under node
# (needs `rustup target add wasm32-unknown-unknown` and wasm-bindgen-cli at
# the version syn-wasm pins — .cargo/config.toml names it as the runner).
cargo clippy -p syn-wasm --target wasm32-unknown-unknown --all-targets -- -D warnings
cargo test -p syn-wasm --release --target wasm32-unknown-unknown
