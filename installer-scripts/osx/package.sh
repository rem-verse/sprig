#!/usr/bin/env bash

set -euo pipefail

ARCH="intel"
if [[ "$1" == "arm" ]]; then
  ARCH="arm"
fi
echo "Building Mac Package for: [$ARCH]"

export SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"
(rm -rf ./working-dir || true)
(rm -rf ./working-dir-pkg || true)

echo "Preparing Directory to Package..."
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
cp ../../../target/release/pcfsserver ./
cp ../../../target/release/dbg-generate-sata-wal-from-pcap ./
cp ../../../pkg/cat-dev/licenses/serial2-tokio-rs-apache.md ./
cp ../../../pkg/cat-dev/licenses/serial2-tokio-rs-bsd.md ./
cp ../../../LICENSE ./
cd ../
echo "Done! Building...."

pkgbuild --root ./working-dir/ --identifier "dev.rem-verse.sprig" --version "0.0.11" --install-location "/usr/local/bin" sprig.pkg

echo "Done! Preparing Distribution Directory..."
mkdir working-dir-pkg
cp "./distribution.${ARCH}.xml" "./working-dir-pkg/distribution.xml"
cp "./sprig.pkg" "./working-dir-pkg/sprig.pkg"
echo "Done! Building!"

cd "./working-dir-pkg"
productbuild --synthesize --package "sprig.pkg" sprig.dist