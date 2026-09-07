#!/bin/sh
# Produces a drag-to-Applications DMG and a ZIP. Publishing is a separate step.
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
unsigned=false
case "${1:-}" in --unsigned) unsigned=true ;; '') ;; *) echo 'Usage: desktop/package.sh [--unsigned]' >&2; exit 2;; esac
if [ "$unsigned" = false ]; then
    : "${IMG_SIGNING_IDENTITY:?A Developer ID Application identity is required. Use --unsigned for a local development package.}"
    : "${IMG_SIGNING_TEAM:?A Developer ID team identifier is required.}"
    : "${IMG_NOTARY_PROFILE:?A notarytool keychain profile is required.}"
    case "$IMG_SIGNING_IDENTITY" in 'Developer ID Application:'*) ;; *) echo 'A Developer ID Application signing identity is required.' >&2; exit 2;; esac
else
    IMG_SIGNING_IDENTITY=-; export IMG_SIGNING_IDENTITY
    unset IMG_SIGNING_TEAM
fi
arch=${IMG_ARCH:-$(uname -m)}
case "$arch" in aarch64) arch=arm64;; arm64|x86_64) ;; *) exit 2;; esac
export IMG_ARCH="$arch"
version=$(python3 -c 'import re; print(re.search(r"^version = \"([^\"]+)\"", open("Cargo.toml").read(), re.M)[1])')
if [ "${GITHUB_REF_TYPE:-}" = tag ] && [ "$GITHUB_REF_NAME" != "desktop-v$version" ]; then
    echo 'Desktop release tag must match workspace Cargo.toml.' >&2; exit 2
fi
app_dir="$project_dir/target/desktop/$arch/Img.app"
export IMG_APP_OUTPUT="$app_dir"
./desktop/bundle.sh release
output_dir="$project_dir/dist/desktop/$arch"
mkdir -p "$output_dir"
staging=$(mktemp -d "$project_dir/target/img-package.XXXXXX")
trap 'rm -rf "$staging"' EXIT HUP INT TERM
# Notarize and staple the app first, so the ZIP also works offline.
if [ "$unsigned" = false ]; then
    ditto -c -k --keepParent "$app_dir" "$staging/notarize.zip"
    xcrun notarytool submit "$staging/notarize.zip" --keychain-profile "$IMG_NOTARY_PROFILE" --wait --output-format json > "$output_dir/app-notarization.json"
    python3 - "$output_dir/app-notarization.json" <<'PY'
import json, sys
if json.load(open(sys.argv[1])).get('status') != 'Accepted': raise SystemExit('App notarization was not accepted.')
PY
    xcrun stapler staple "$app_dir"
    xcrun stapler validate "$app_dir"
    spctl --assess --type execute --verbose=2 "$app_dir"
fi
name="img-desktop_${version}_macos_${arch}"
ditto -c -k --keepParent "$app_dir" "$output_dir/$name.zip"
mkdir -p "$staging/volume"
ditto "$app_dir" "$staging/volume/Img.app"
ln -s /Applications "$staging/volume/Applications"
hdiutil create -volname "img $version" -srcfolder "$staging/volume" -ov -format UDZO "$output_dir/$name.dmg"
if [ "$unsigned" = false ]; then
    codesign --force --timestamp --sign "$IMG_SIGNING_IDENTITY" "$output_dir/$name.dmg"
    xcrun notarytool submit "$output_dir/$name.dmg" --keychain-profile "$IMG_NOTARY_PROFILE" --wait --output-format json > "$output_dir/dmg-notarization.json"
    python3 - "$output_dir/dmg-notarization.json" <<'PY'
import json, sys
if json.load(open(sys.argv[1])).get('status') != 'Accepted': raise SystemExit('DMG notarization was not accepted.')
PY
    xcrun stapler staple "$output_dir/$name.dmg"
    xcrun stapler validate "$output_dir/$name.dmg"
fi
(cd "$output_dir" && shasum -a 256 "$name.dmg" > "$name.dmg.sha256" && shasum -a 256 "$name.zip" > "$name.zip.sha256")
python3 - "$output_dir/build-info.json" "$version" "$arch" "$unsigned" <<'PY'
import json, sys
path, version, arch, unsigned = sys.argv[1:]
with open(path, 'w') as f: json.dump(dict(version=version, arch=arch, notarized=unsigned!='true', channel='development' if unsigned=='true' else 'release'), f, indent=2)
PY
printf 'Packages: %s\n' "$output_dir"
