#!/usr/bin/env bash

set -eo pipefail

export SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

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
if ! shellcheck --help >/dev/null 2>&1 ; then
  die "Missing \`shellcheck\` CLI Tool" "note: you can find info about how to install it at it's website <https://www.shellcheck.net/>"
fi

cd "${SCRIPT_DIR}/../../"
shfmt --diff --posix "./cmd/getbridge/sh/getbridge"
shfmt --diff --posix "./cmd/getbridgetype/sh/getbridgetype"
shellcheck -x --check-sourced --shell=sh --severity=style "./cmd/getbridge/sh/getbridge"
shellcheck -x --check-sourced --shell=sh --severity=style "./cmd/getbridgetype/sh/getbridgetype"
shellcheck -x --check-sourced --shell=sh --severity=style "./cmd/setbridge/sh/setbridge"