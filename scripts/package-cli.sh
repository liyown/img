#!/bin/sh
set -eu
project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_dir"
package_python=${IMG_PYTHON:-python3}
if ! command -v "$package_python" >/dev/null 2>&1; then package_python=python; fi
target=${IMG_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}
case "$target" in
    aarch64-apple-darwin) os=darwin; arch=arm64; ext= ;;
    x86_64-apple-darwin) os=darwin; arch=amd64; ext= ;;
    x86_64-unknown-linux-gnu|x86_64-unknown-linux-musl) os=linux; arch=amd64; ext= ;;
    aarch64-unknown-linux-gnu|aarch64-unknown-linux-musl) os=linux; arch=arm64; ext= ;;
    x86_64-pc-windows-msvc) os=windows; arch=amd64; ext=.exe ;;
    *) echo "Unsupported CLI release target: $target" >&2; exit 2;;
esac
export IMG_BUILD_COMMIT="$(git rev-parse --short HEAD)"
export IMG_BUILD_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
version=$("$package_python" -c 'import re; print(re.search(r"^version = \"([^\"]+)\"", open("Cargo.toml").read(), re.M)[1])')
if [ "${GITHUB_REF_TYPE:-}" = tag ] && [ "$GITHUB_REF_NAME" != "v$version" ]; then
    echo 'CLI release tag must match the workspace version.' >&2; exit 2
fi
cargo build --locked --release -p img-cli --target "$target"
output="$project_dir/dist/cli/$target"
mkdir -p "$output"
staging=$(mktemp -d "$project_dir/target/img-cli-package.XXXXXX")
trap 'rm -rf "$staging"' EXIT HUP INT TERM
cp "target/$target/release/img$ext" "$staging/img$ext"
cp README.md "$staging/README.md"
if [ "$os" = darwin ]; then codesign --force --sign - "$staging/img"; fi
if [ "$os" = windows ]; then
    "$package_python" - "$staging" "$output/img_${os}_${arch}.zip" <<'PY'
import pathlib, sys, zipfile
with zipfile.ZipFile(sys.argv[2], 'w', zipfile.ZIP_DEFLATED) as z:
    for p in pathlib.Path(sys.argv[1]).iterdir(): z.write(p, p.name)
PY
    asset="img_${os}_${arch}.zip"
else
    asset="img_${os}_${arch}.tar.gz"
    tar -czf "$output/$asset" -C "$staging" "img$ext" README.md
fi
"$package_python" - "$output" "$asset" "$version" "$target" <<'PY'
import hashlib, json, pathlib, sys
out=pathlib.Path(sys.argv[1]); asset, version, target=sys.argv[2:]
(out/'checksums.txt').write_text(hashlib.sha256((out/asset).read_bytes()).hexdigest()+'  '+asset+'\n')
(out/'build-info.json').write_text(json.dumps(dict(version=version,target=target,implementation='Rust',product='cli'),indent=2)+'\n')
PY
printf 'CLI package: %s/%s\n' "$output" "$asset"
