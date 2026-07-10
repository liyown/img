#!/bin/sh
set -eu

REPO="${IMG_REPO:-liyown/img}"
VERSION="${IMG_VERSION:-latest}"

fail() {
  echo "img installer: $*" >&2
  exit 1
}

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

case "$(uname -s)" in
  Darwin) os="darwin" ;;
  Linux) os="linux" ;;
  *) fail "unsupported operating system: $(uname -s)" ;;
esac

case "$(uname -m)" in
  x86_64|amd64) arch="amd64" ;;
  arm64|aarch64) arch="arm64" ;;
  *) fail "unsupported architecture: $(uname -m)" ;;
esac

asset="img_${os}_${arch}.tar.gz"
if [ "$VERSION" = "latest" ]; then
  base="https://github.com/${REPO}/releases/latest/download"
else
  case "$VERSION" in v*) tag="$VERSION" ;; *) tag="v$VERSION" ;; esac
  base="https://github.com/${REPO}/releases/download/${tag}"
fi

tmp="$(mktemp -d 2>/dev/null || mktemp -d -t img-install)"
trap 'rm -rf "$tmp"' EXIT INT TERM

echo "Downloading ${asset}..."
curl -fsSL "${base}/${asset}" -o "${tmp}/${asset}"
curl -fsSL "${base}/checksums.txt" -o "${tmp}/checksums.txt"

expected="$(awk -v name="$asset" '$2 == name { print $1 }' "${tmp}/checksums.txt")"
[ -n "$expected" ] || fail "checksum for ${asset} was not found"

if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${tmp}/${asset}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  actual="$(shasum -a 256 "${tmp}/${asset}" | awk '{print $1}')"
else
  fail "sha256sum or shasum is required"
fi
[ "$actual" = "$expected" ] || fail "checksum verification failed"

tar -xzf "${tmp}/${asset}" -C "$tmp"
[ -f "${tmp}/img" ] || fail "release archive does not contain img"

if [ -n "${IMG_INSTALL_DIR:-}" ]; then
  install_dir="$IMG_INSTALL_DIR"
elif [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
  install_dir="/usr/local/bin"
else
  install_dir="${HOME}/.local/bin"
fi

mkdir -p "$install_dir"
install -m 755 "${tmp}/img" "${install_dir}/img"

echo "Installed img to ${install_dir}/img"
case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *) echo "Add ${install_dir} to PATH before running img." ;;
esac
"${install_dir}/img" version
