#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY_NAME="discuss"
SOURCE_BINARY="${SCRIPT_DIR}/target/release/${BINARY_NAME}"
INSTALL_DIR="${HOME}/.discuss/bin"
LINK_DIR="${HOME}/.local/bin"
INSTALLED_BINARY="${INSTALL_DIR}/${BINARY_NAME}"
LINK_BINARY="${LINK_DIR}/${BINARY_NAME}"

if [[ ! -f "${SCRIPT_DIR}/Cargo.toml" ]]; then
  printf 'error: install.sh must be run from a discuss source checkout with Cargo.toml next to the script\n' >&2
  printf 'hint: clone the repository first, then run ./install.sh from the checkout\n' >&2
  exit 1
fi

cd "${SCRIPT_DIR}"

printf 'Building %s with warnings denied...\n' "${BINARY_NAME}"
RUSTFLAGS="-D warnings" cargo build --release

mkdir -p "${INSTALL_DIR}" "${LINK_DIR}"
install -m 0755 "${SOURCE_BINARY}" "${INSTALLED_BINARY}"
ln -sfn "${INSTALLED_BINARY}" "${LINK_BINARY}"

printf 'Installed %s to %s\n' "${BINARY_NAME}" "${INSTALLED_BINARY}"
printf 'Linked %s to %s\n' "${LINK_BINARY}" "${INSTALLED_BINARY}"
"${LINK_BINARY}" --version

if command -v "${BINARY_NAME}" >/dev/null 2>&1; then
  "${BINARY_NAME}" --version
else
  printf 'warning: %s is not on PATH; add %s to PATH to run `%s` directly\n' "${LINK_DIR}" "${LINK_DIR}" "${BINARY_NAME}" >&2
fi
