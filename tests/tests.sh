#!/bin/bash

## NOTE : Should be called from / (in this directory, not the Linux root)
## Command : `./tests/tests.sh`

set -e

# This `cargo install --path .` is not used here because it's to slow for only
# simple tests. The "target/debug/amargo" will be used
BIN=target/debug/amargo

# Remove already-generate tests
shopt -s extglob
(cd tests/ && rm -rf !(tests.sh))
shopt -u extglob

# Build the project to "$BIN"
cargo build

set +e

# Tests for C
$BIN new tests/c_binary
(cd tests/c_binary && ../../$BIN build)
echo "-------------------------------------------------------------------------"
$BIN new tests/c_dylib -- dynamic
(cd tests/c_dylib && ../../$BIN build)
echo "-------------------------------------------------------------------------"

# Tests for C++ (not currently available)
