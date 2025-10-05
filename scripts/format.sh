#!/usr/bin/env bash

cargo clippy --fix --allow-dirty --allow-staged
cargo clippy --fix --allow-dirty --all-targets --all-features -- -D warnings
cargo fmt
