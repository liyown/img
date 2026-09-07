#!/bin/sh
# Install either the standalone Rust CLI or the macOS GUI (including CLI).
set -eu
repo=${IMG_REPO:-liyown/img}
version=${IMG_VERSION:-latest}
product=${IMG_PRODUCT:-cli}
case "${1:-}" in
  --cli|cli) product=cli ;;
  --gui|gui) product=gui ;;
  --help|-h) echo 'Usage: install.sh [--cli|--gui]'; echo 'Environment: IMG_VERSION, IMG_INSTALL_DIR, IMG_APP_DIR, IMG_LOCAL_PACKAGE_DIR'; exit 0 ;;
  '') ;;
  *) echo 'Usage: install.sh [--cli|--gui]' >&2; exit 2 ;;
esac
fail() { echo "img installer: $*" >&2; exit 1; }
case "$product" in cli|gui) ;; *) fail 'choose cli or gui';; esac
case "$(uname -s)" in Darwin) os=darwin;; Linux) os=linux;; *) fail 'use install.ps1 on Windows';; esac
case "$(uname -m)" in x86_64|amd64) arch=amd64; gui_arch=x86_64;; arm64|aarch64) arch=arm64; gui_arch=arm64;; *) fail 'unsupported architecture';; esac
if [ "$product" = gui ] && [ "$os" = linux ] && [ "$arch" != amd64 ]; then fail 'Linux GUI packages currently require x86_64; ARM64 can use --cli'; fi
if [ -z "${IMG_LOCAL_PACKAGE_DIR:-}" ]; then command -v curl >/dev/null 2>&1 || fail 'curl is required'; fi
install_dir=${IMG_INSTALL_DIR:-"$HOME/.local/bin"}
mkdir -p "$install_dir"
install_dir=$(CDPATH= cd -- "$install_dir" && pwd)
[ ! -d "$install_dir/img" ] || fail 'CLI destination is a directory'
tmp=$(mktemp -d)
cli_stage=''
staged_app=''
backup_app=''
gui_dest=''
cleanup() {
  if [ -n "$backup_app" ] && [ -d "$backup_app" ] && [ ! -e "$gui_dest" ]; then mv "$backup_app" "$gui_dest"; fi
  [ -z "$staged_app" ] || rm -rf "$staged_app"
  [ -z "$cli_stage" ] || rm -rf "$cli_stage"
  rm -rf "$tmp"
}
trap cleanup EXIT HUP INT TERM
fetch_asset() {
  if [ -n "${IMG_LOCAL_PACKAGE_DIR:-}" ]; then cp "$IMG_LOCAL_PACKAGE_DIR/$1" "$tmp/$1";
  else curl --proto '=https' --proto-redir '=https' -fsSL "$base/$1" -o "$tmp/$1"; fi
}
if [ "$product" = cli ]; then
  asset="img_${os}_${arch}.tar.gz"
  if [ "$version" = latest ]; then base="https://github.com/$repo/releases/latest/download";
  else version=${version#v}; base="https://github.com/$repo/releases/download/v$version"; fi
  checksum=checksums.txt
else
  version=${version#desktop-v}; version=${version#v}
  if [ "$version" = latest ]; then
    [ -z "${IMG_LOCAL_PACKAGE_DIR:-}" ] || fail 'set IMG_VERSION when installing a local GUI package'
    curl --proto '=https' --proto-redir '=https' -fsSL "https://github.com/$repo/releases.atom" -o "$tmp/releases.atom"
    version=$(sed -nE 's@.*releases/tag/desktop-v([0-9]+\.[0-9]+\.[0-9]+)".*@\1@p' "$tmp/releases.atom" | head -1)
  fi
  printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || fail 'no stable GUI release found; set IMG_VERSION to a published desktop version'
  if [ "$os" = darwin ]; then asset="img-desktop_${version}_macos_${gui_arch}.zip";
  else asset="img-desktop_${version}_linux_x86_64.deb"; fi
  checksum="$asset.sha256"
  base="https://github.com/$repo/releases/download/desktop-v$version"
fi
fetch_asset "$asset"
fetch_asset "$checksum"
expected=$(awk -v name="$asset" '$2 == name { print $1 }' "$tmp/$checksum")
[ -n "$expected" ] || fail 'package checksum not found'
if command -v sha256sum >/dev/null 2>&1; then actual=$(sha256sum "$tmp/$asset" | awk '{print $1}');
elif command -v shasum >/dev/null 2>&1; then actual=$(shasum -a 256 "$tmp/$asset" | awk '{print $1}');
else fail 'sha256sum or shasum is required'; fi
[ "$actual" = "$expected" ] || fail 'checksum verification failed'
if [ "$product" = gui ] && [ "$os" = linux ]; then
  command -v apt-get >/dev/null 2>&1 || fail 'GUI installation requires Ubuntu 24.04 / Debian 13 or newer with apt-get'
  if [ "$(id -u)" = 0 ]; then apt-get install --yes "$tmp/$asset";
  else sudo apt-get install --yes "$tmp/$asset"; fi
  echo 'Installed GUI and bundled CLI. Start img from the application menu.'
  exit 0
fi
cli_stage=$(mktemp -d "$install_dir/.img-install.XXXXXX")
if [ "$product" = cli ]; then
  tar -xzf "$tmp/$asset" -C "$tmp" img
  [ -f "$tmp/img" ] && [ ! -L "$tmp/img" ] || fail 'package has no standalone img binary'
  install -m 755 "$tmp/img" "$cli_stage/img"
else
  app_dir=${IMG_APP_DIR:-"$HOME/Applications"}
  mkdir -p "$app_dir"
  app_dir=$(CDPATH= cd -- "$app_dir" && pwd)
  gui_dest="$app_dir/Img.app"
  if [ -e "$gui_dest" ] && lsof -t "$gui_dest/Contents/MacOS/img-desktop" >/dev/null 2>&1; then fail 'quit img before replacing an installed GUI'; fi
  ditto -x -k "$tmp/$asset" "$tmp/unpacked"
  app="$tmp/unpacked/Img.app"
  [ -x "$app/Contents/MacOS/img" ] && [ -x "$app/Contents/MacOS/img-desktop" ] || fail 'GUI package is missing the bundled CLI'
  codesign --verify --deep --strict "$app" || fail 'invalid application signature'
  staged_app=$(mktemp -d "$app_dir/.img-app.XXXXXX")
  ditto "$app" "$staged_app/Img.app"
  if [ -e "$gui_dest" ]; then
    backup_app="$staged_app/Previous.app"
    mv "$gui_dest" "$backup_app"
  fi
  mv "$staged_app/Img.app" "$gui_dest"
  ln -s "$gui_dest/Contents/MacOS/img" "$cli_stage/img"
  echo "Installed GUI: $gui_dest"
fi
mv -f "$cli_stage/img" "$install_dir/img"
echo "Installed CLI: $install_dir/img"
"$install_dir/img" version
case ":$PATH:" in *":$install_dir:"*) ;; *) printf 'Add this directory to PATH: %s\n' "$install_dir";; esac
