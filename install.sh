#!/usr/bin/env bash
# One-line install: curl -fsSL https://github.com/iam2r/apmux/releases/latest/download/install.sh | bash
set -Eeuo pipefail

REPO="${APMUX_REPO:-iam2r/apmux}"
BIN_NAME="apmux"
INSTALL_DIR="${APMUX_INSTALL_DIR:-$HOME/.local/bin}"
TARGET="${INSTALL_DIR}/${BIN_NAME}"
RELEASES_URL="https://github.com/${REPO}/releases"
SKIP_PATH="${APMUX_SKIP_PATH:-0}"
VERSION="${1:-latest}"
[[ "${VERSION}" == "latest" || "${VERSION}" =~ ^v ]] || VERSION="v${VERSION}"

TMP_DIR=""
ASSET_NAME=""
EXTRACT_KIND="tar"

info() { printf '  \033[1;32minfo\033[0m: %s\n' "$*"; }
warn() { printf '  \033[1;33mwarn\033[0m: %s\n' "$*" >&2; }
err() { printf '  \033[1;31merror\033[0m: %s\n' "$*" >&2; }

cleanup() {
  if [[ -n "${TMP_DIR}" && -d "${TMP_DIR}" ]]; then
    rm -rf "${TMP_DIR}"
  fi
}

on_error() {
  err "Installation failed (line ${1:-?})"
  err "Manual download: ${RELEASES_URL}"
}

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    err "Required command not found: $1"
    exit 1
  fi
}

detect_asset() {
  local os arch
  os="$(uname -s 2>/dev/null || true)"
  arch="$(uname -m 2>/dev/null || true)"
  ASSET_NAME=""
  EXTRACT_KIND="tar"

  case "${os}" in
    Darwin)
      ASSET_NAME="apmux-darwin-universal.tar.gz"
      ;;
    Linux)
      case "${arch}" in
        x86_64 | amd64) ASSET_NAME="apmux-linux-x64-musl.tar.gz" ;;
        aarch64 | arm64) ASSET_NAME="apmux-linux-arm64-musl.tar.gz" ;;
        *)
          err "Unsupported Linux architecture: ${arch}"
          err "See ${RELEASES_URL}"
          exit 1
          ;;
      esac
      ;;
    MINGW* | MSYS* | CYGWIN*)
      ASSET_NAME="apmux-windows-x64.zip"
      EXTRACT_KIND="zip"
      BIN_NAME="apmux.exe"
      TARGET="${INSTALL_DIR}/${BIN_NAME}"
      ;;
    *)
      err "Unsupported OS: ${os}"
      err "Windows PowerShell: irm https://github.com/${REPO}/releases/latest/download/install.ps1 | iex"
      err "See ${RELEASES_URL}"
      exit 1
      ;;
  esac
}

download_asset() {
  local url="$1"
  local dest="$2"
  if command -v curl >/dev/null 2>&1; then
    curl --fail --location --silent --show-error --output "${dest}" "${url}"
  elif command -v wget >/dev/null 2>&1; then
    wget --quiet --output-document="${dest}" "${url}"
  else
    err "Need curl or wget"
    exit 1
  fi
}

download() {
  local url dest
  if [[ "${VERSION}" == "latest" ]]; then
    url="${RELEASES_URL}/latest/download/${ASSET_NAME}"
  else
    url="${RELEASES_URL}/download/${VERSION}/${ASSET_NAME}"
  fi
  dest="${TMP_DIR}/${ASSET_NAME}"
  info "Downloading ${ASSET_NAME}"
  if download_asset "${url}" "${dest}"; then
    return 0
  fi
  rm -f "${dest}"
  err "Unable to download ${url}"
  err "No binary was installed."
  exit 1
}

extract() {
  info "Extracting archive"
  if [[ "${EXTRACT_KIND}" == "zip" ]]; then
    if command -v unzip >/dev/null 2>&1; then
      unzip -qo "${TMP_DIR}/${ASSET_NAME}" -d "${TMP_DIR}"
    else
      tar -xf "${TMP_DIR}/${ASSET_NAME}" -C "${TMP_DIR}"
    fi
  else
    LC_ALL=C tar -xzf "${TMP_DIR}/${ASSET_NAME}" -C "${TMP_DIR}"
  fi
  if [[ ! -f "${TMP_DIR}/${BIN_NAME}" ]]; then
    err "Binary '${BIN_NAME}' not found in archive."
    exit 1
  fi
}

install_binary() {
  local staged="${TARGET}.new"
  mkdir -p "${INSTALL_DIR}"
  rm -f "${staged}"
  cp "${TMP_DIR}/${BIN_NAME}" "${staged}"
  chmod 755 "${staged}"
  mv -f "${staged}" "${TARGET}"
  chmod 755 "${TARGET}"
  if [[ "$(uname -s)" == "Darwin" ]] && command -v xattr >/dev/null 2>&1; then
    xattr -cr "${TARGET}" 2>/dev/null || true
  fi
}

rc_for_shell() {
  local shell_name
  shell_name="$(basename "${SHELL:-bash}")"
  case "${shell_name}" in
    zsh) printf '%s\n%s\n' "${HOME}/.zshrc" "export PATH=\"${INSTALL_DIR}:\$PATH\"" ;;
    fish) printf '%s\n%s\n' "${HOME}/.config/fish/config.fish" "fish_add_path ${INSTALL_DIR}" ;;
    *) printf '%s\n%s\n' "${HOME}/.bashrc" "export PATH=\"${INSTALL_DIR}:\$PATH\"" ;;
  esac
}

ensure_path() {
  local resolved
  resolved="$(command -v "${BIN_NAME%%.exe}" 2>/dev/null || command -v "${BIN_NAME}" 2>/dev/null || true)"
  if [[ -n "${resolved}" && "${resolved}" != "${TARGET}" ]]; then
    warn "${BIN_NAME} currently resolves to ${resolved}, so ${TARGET} may be shadowed."
  fi

  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) return 0 ;;
  esac

  if [[ "${SKIP_PATH}" == "1" ]]; then
    warn "${INSTALL_DIR} is not in PATH (APMUX_SKIP_PATH=1; not modifying shell rc)"
    return 0
  fi

  local rc cmd
  rc="$(rc_for_shell | sed -n '1p')"
  cmd="$(rc_for_shell | sed -n '2p')"
  mkdir -p "$(dirname "${rc}")"
  touch "${rc}"
  if grep -Fqs "# aimux PATH" "${rc}"; then
    # In-place upgrade of the pre-rename managed block marker so old installs
    # don't accumulate a second block.
    sed -i "s/# aimux PATH/# apmux PATH/g; s/# end aimux PATH/# end apmux PATH/g" "${rc}"
    info "${INSTALL_DIR} is not in this shell's PATH; upgraded the managed block in ${rc}"
  elif grep -Fqs "# apmux PATH" "${rc}"; then
    info "${INSTALL_DIR} is not in this shell's PATH; a managed block already exists in ${rc}"
  else
    {
      printf '\n# apmux PATH\n'
      printf '%s\n' "${cmd}"
      printf '# end apmux PATH\n'
    } >>"${rc}"
    info "Added ${INSTALL_DIR} to PATH in ${rc}"
  fi
  printf '  Open a new terminal, or run:\n\n    %s\n\n' "${cmd}"
}

main() {
  trap cleanup EXIT
  trap 'on_error "${LINENO}"' ERR
  need_cmd uname
  need_cmd tar
  need_cmd mktemp
  detect_asset
  TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/apmux-install.XXXXXX")"
  download
  extract
  install_binary
  info "Installed ${BIN_NAME} to ${TARGET}"
  ensure_path
  printf '  Run \033[1m%s --version\033[0m to verify.\n\n' "${BIN_NAME%%.exe}"
}

main "$@"
