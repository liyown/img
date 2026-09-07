#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
profile=${1:-debug}
case "$profile" in debug|release) ;; *) echo 'Usage: desktop/bundle.sh [debug|release]' >&2; exit 2;; esac
arch=${IMG_ARCH:-$(uname -m)}
case "$arch" in
    arm64|aarch64) arch=arm64; rust_target=aarch64-apple-darwin ;;
    x86_64) rust_target=x86_64-apple-darwin ;;
    *) echo 'Only macOS arm64 and x86_64 are supported.' >&2; exit 2 ;;
esac
version=$(python3 -c 'import re; print(re.search(r"^version = \"([^\"]+)\"", open("Cargo.toml").read(), re.M)[1])')
export IMG_BUILD_COMMIT="$(git rev-parse --short HEAD)"
export IMG_BUILD_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
if [ "$profile" = release ]; then export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-13.0}; fi
if [ "$profile" = release ]; then
    cargo build --locked --release -p img-desktop -p img-cli --target "$rust_target"
    binary_dir="$project_dir/target/$rust_target/release"
    app_dir=${IMG_APP_OUTPUT:-"$project_dir/target/desktop/$arch/Img.app"}
else
    cargo build --locked -p img-desktop -p img-cli
    binary_dir="$project_dir/target/debug"
    app_dir=${IMG_APP_OUTPUT:-"$project_dir/target/Img.app"}
fi
rm -f "$app_dir/Contents/MacOS/img-engine"
mkdir -p "$app_dir/Contents/MacOS" "$app_dir/Contents/Resources"
cp "$binary_dir/img-desktop" "$binary_dir/img" "$app_dir/Contents/MacOS/"
python3 - "$app_dir" "$version" "${MACOSX_DEPLOYMENT_TARGET:-13.0}" <<'PY'
import plistlib, pathlib, sys
app, version, minimum = sys.argv[1:]
plist = dict(CFBundleIdentifier='dev.img.desktop', CFBundleName='img', CFBundleDisplayName='img',
    CFBundleExecutable='img-desktop', CFBundleIconFile='Img.icns', CFBundlePackageType='APPL',
    CFBundleShortVersionString=version, CFBundleVersion=version, LSMinimumSystemVersion=minimum,
    NSHighResolutionCapable=True, NSPrincipalClass='NSApplication')
with open(pathlib.Path(app)/'Contents/Info.plist', 'wb') as f: plistlib.dump(plist, f)
PY
iconset_dir=$(mktemp -d "$project_dir/target/img-icon.XXXXXX")
trap 'rm -rf "$iconset_dir"' EXIT HUP INT TERM
mkdir -p "$iconset_dir/Img.iconset"
for icon_size in 16 32 128 256 512; do
    sips -z "$icon_size" "$icon_size" desktop/assets/img-mark.png --out "$iconset_dir/Img.iconset/icon_${icon_size}x${icon_size}.png" >/dev/null
    icon_double=$((icon_size * 2))
    sips -z "$icon_double" "$icon_double" desktop/assets/img-mark.png --out "$iconset_dir/Img.iconset/icon_${icon_size}x${icon_size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset_dir/Img.iconset" -o "$app_dir/Contents/Resources/Img.icns"
cp desktop/INSTALL.html "$app_dir/Contents/Resources/INSTALL.html"
identity=${IMG_SIGNING_IDENTITY:--}
if [ "$identity" = '-' ]; then
    codesign --force --sign - "$app_dir/Contents/MacOS/img"
    codesign --force --sign - "$app_dir"
else
    : "${IMG_SIGNING_TEAM:?Set the Developer ID team identifier before building a release.}"
    codesign --force --options runtime --timestamp --sign "$identity" "$app_dir/Contents/MacOS/img"
    codesign --force --options runtime --timestamp --sign "$identity" "$app_dir"
fi
codesign --verify --deep --strict "$app_dir"
printf '%s\n' "$app_dir"
