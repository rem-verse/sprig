#!/usr/bin/env bash

set -euo pipefail

export SCRIPT_DIR=$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )

die() {
  echo "ERROR: $1" >&2
  shift
  while [[ -n $1 ]]; do
    echo "    $1" >&2
    shift
  done
  exit 1
}

if ! shfmt --help >/dev/null 2>&1 ; then
  die "Missing \`shfmt\` CLI Tool" "note: you can find info about how to install it at it's hosted repository <https://github.com/mvdan/sh>"
fi

shfmt --case-indent --indent 0 --write --posix "${SCRIPT_DIR}/../../cmd/getbridge/sh/getbridge"
shfmt --case-indent --indent 0 --write --posix "${SCRIPT_DIR}/../../cmd/getbridgetype/sh/getbridgetype"
shfmt --case-indent --indent 0 --write --posix "${SCRIPT_DIR}/../../cmd/setbridge/sh/setbridge"