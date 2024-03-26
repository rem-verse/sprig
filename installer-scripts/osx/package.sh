#!/usr/bin/env bash

set -euo pipefail

export SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"
(rm -rf ./working-dir || true)
mkdir working-dir
cd working-dir
cp ../../../target/release/catlog ./
cp ../../../target/release/bridgectl ./
cp ../../../target/release/findbridge ./
cp ../../../target/release/getbridgeconfig ./
cp ../../../target/release/setbridgeconfig ./
cp ../../../cmd/getbridge/sh/getbridge ./
cp ../../../cmd/getbridgetype/sh/getbridgetype ./
cp ../../../cmd/setbridge/sh/setbridge ./
cp ../../../target/release/mionps ./
cp ../../../target/release/mionparamspace ./
cp ../../../pkg/cat-dev/licenses/serial2-tokio-rs-apache.md ./
cp ../../../pkg/cat-dev/licenses/serial2-tokio-rs-bsd.md ./
cp ../../../LICENSE ./
cd ../
pkgbuild --root ./working-dir/ --identifier "dev.rem-verse.sprig" --version "0.0.5" --install-location "/usr/local/bin" sprig.pkg
productbuild --synthesize --package "sprig.pkg" sprig.dist