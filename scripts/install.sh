#!/bin/sh
# Install a prebuilt InfoTransfer release. scripts/release.py fills in the version, the
# checksum and the signing requirement.
#
# Everything runs inside main, called on the last line, so a download cut short by
# `curl | sh` runs nothing.
set -eu

VERSION='@VERSION@'
SHA256='@SHA256@'
REQUIREMENT='@REQUIREMENT@'
URL="https://github.com/sxxrfx/infotransfer/releases/download/v$VERSION/InfoTransfer-$VERSION-arm64.zip"
SOURCE='https://github.com/sxxrfx/infotransfer'

fail() { printf '\nInfoTransfer: %s\n' "$1" >&2; exit 1; }

main() {

[ "$(uname -s)" = Darwin ] || fail 'InfoTransfer runs on macOS only.'
[ "$(uname -m)" = arm64 ] || fail 'InfoTransfer needs a Mac with Apple Silicon (M1 or newer).'
major=$(sw_vers -productVersion | cut -d. -f1)
[ "$major" -ge 14 ] || fail 'InfoTransfer needs macOS 14 Sonoma or later.'

cat <<EOF

  InfoTransfer $VERSION

  WARNING: this is an unsigned app.
  It is not signed with an Apple Developer ID and not notarized by Apple,
  so macOS has not checked it. This script installs it without a Gatekeeper check.

  Downloading the source and compiling your own copy is more secure,
  because you can read the code you run:
  $SOURCE#build-from-source

EOF

if [ "${INFOTRANSFER_YES:-}" != 1 ]; then
    [ -r /dev/tty ] || fail 'Run this in a terminal, or set INFOTRANSFER_YES=1 to install without asking.'
    printf '  Install the prebuilt app anyway? [y/N] '
    read -r answer </dev/tty || answer=
    case "$answer" in y | Y | yes | YES) ;; *) fail 'Nothing was installed.' ;; esac
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

printf '\nDownloading %s\n' "$URL"
curl -fL --progress-bar -o "$work/InfoTransfer.zip" "$URL" || fail 'Download failed.'
actual=$(shasum -a 256 "$work/InfoTransfer.zip" | cut -d' ' -f1)
[ "$actual" = "$SHA256" ] || fail "Checksum mismatch (expected $SHA256, got $actual). Nothing was installed."
echo 'Checksum verified.'

ditto -x -k "$work/InfoTransfer.zip" "$work/unpacked"
app="$work/unpacked/InfoTransfer.app"
[ -d "$app" ] || fail 'The download did not contain InfoTransfer.app.'
codesign --verify --deep --strict "$app" 2>/dev/null || fail 'The app signature is broken. Nothing was installed.'
# The same identity signs every release, which also keeps the Accessibility permission.
codesign --verify --strict "-R=$REQUIREMENT" "$app" 2>/dev/null ||
    fail 'The app is signed by an unexpected identity. Nothing was installed.'

target=/Applications
[ -w "$target" ] || target="$HOME/Applications"
mkdir -p "$target"

if pgrep -x infotransfer >/dev/null 2>&1; then
    echo 'Quitting the running InfoTransfer.'
    pkill -x infotransfer || true
    sleep 1
fi

# Copy next to the old app first, so a failed copy leaves the installed app untouched.
new="$target/.InfoTransfer.app.new"
old="$target/.InfoTransfer.app.old"
rm -rf "$new" "$old"
ditto "$app" "$new" || { rm -rf "$new"; fail 'Copying the app failed. Nothing was changed.'; }
if [ -e "$target/InfoTransfer.app" ]; then
    mv "$target/InfoTransfer.app" "$old"
fi
if ! mv "$new" "$target/InfoTransfer.app"; then
    [ -e "$old" ] && mv "$old" "$target/InfoTransfer.app"
    rm -rf "$new"
    fail 'Replacing the app failed. The previous version was kept.'
fi
rm -rf "$old"
# curl does not quarantine downloads; clear the flag anyway in case a proxy or tool added it.
xattr -dr com.apple.quarantine "$target/InfoTransfer.app" 2>/dev/null || true

cat <<EOF

Installed $target/InfoTransfer.app

Next:
  1. InfoTransfer opens now. Click "Download voices" in its window.
  2. Click "Open settings" next to Accessibility and turn InfoTransfer on.
  3. Select text in any app and tap the right Option key.

EOF
open "$target/InfoTransfer.app"
}

main "$@"
