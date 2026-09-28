#!/usr/bin/env bash
# The `--` keeps our own flags (`--root`, `--port`) from being eaten by cargo.
exec cargo run -- "$@"