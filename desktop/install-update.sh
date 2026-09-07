#!/bin/sh
# Invoked only after app validation and queue persistence. All paths are positional arguments.
set -eu
parent=$1
stage=$2
destination=$3
receipt=$4
version=$5
backup="$stage/Previous.app"
installed=false
finish() {
  result=$?
  trap - EXIT HUP INT TERM
  if [ "$result" -ne 0 ]; then
    if [ "$installed" = true ]; then rm -rf "$destination"; fi
    if [ -d "$backup" ]; then mv "$backup" "$destination"; fi
    printf 'failed\n%s\n' "$version" > "$receipt"
  fi
  rm -rf "$stage"
  exit "$result"
}
trap finish EXIT HUP INT TERM
attempt=0
while kill -0 "$parent" 2>/dev/null; do
  attempt=$((attempt + 1))
  [ "$attempt" -lt 120 ] || exit 1
  sleep 1
done
# Refuse a second app process started while the original was shutting down.
if [ -e "$destination" ] && /usr/sbin/lsof -t "$destination/Contents/MacOS/img-desktop" >/dev/null 2>&1; then exit 1; fi
if [ -e "$destination" ]; then mv "$destination" "$backup"; fi
mv "$stage/Next.app" "$destination"
installed=true
printf 'installed\n%s\n' "$version" > "$receipt"
set -- -n --env "APERTURE_DATA_DIR=$(dirname "$receipt")"
if [ -n "${APERTURE_CONFIG_PATH:-}" ]; then set -- "$@" --env "APERTURE_CONFIG_PATH=$APERTURE_CONFIG_PATH"; fi
/usr/bin/open "$@" "$destination"
