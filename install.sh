#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALL_DIR="${DISCUSS_INSTALL_DIR:-/usr/local/bin}"
BINARY_NAME="discuss"
SOURCE_BINARY="${SCRIPT_DIR}/target/release/${BINARY_NAME}"
INSTALLED_BINARY="${INSTALL_DIR}/${BINARY_NAME}"

if [[ ! -f "${SCRIPT_DIR}/Cargo.toml" ]]; then
  printf 'error: install.sh must be run from a discuss source checkout with Cargo.toml next to the script\n' >&2
  printf 'hint: clone the repository first, then run ./install.sh from the checkout\n' >&2
  exit 1
fi

cd "${SCRIPT_DIR}"

printf 'Building %s with warnings denied...\n' "${BINARY_NAME}"
RUSTFLAGS="-D warnings" cargo build --release

mkdir -p "${INSTALL_DIR}"
cp -p "${SOURCE_BINARY}" "${INSTALLED_BINARY}"

printf 'Installed %s to %s\n' "${BINARY_NAME}" "${INSTALLED_BINARY}"
"${INSTALLED_BINARY}" --version
