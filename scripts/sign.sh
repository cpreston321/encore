#!/bin/bash
# Signs Encore.app with your own certificate, for an app you built or downloaded:
#
#   ./scripts/sign.sh                      # ~/Applications/Encore.app
#   ./scripts/sign.sh /path/to/Encore.app
#
# The identity is, in order: ENCORE_SIGN_IDENTITY (a certificate name, or "-" for ad hoc),
# your Apple Development certificate, your Developer ID Application certificate, or ad hoc.
#
# macOS remembers "Always Allow" for Keychain access per code signature, so signing every
# build and update with the same certificate stops the prompt coming back. A downloaded app
# also loses its quarantine flag here, so Gatekeeper stops asking about it.
set -euo pipefail

app=${1:-"$HOME/Applications/Encore.app"}
identifier=io.github.cpreston321.encore

if [ ! -d "$app/Contents/MacOS" ]; then
  echo "No app at $app" >&2
  exit 1
fi

find_identity() {
  security find-identity -v -p codesigning | sed -n "s/.*\"\($1:[^\"]*\)\".*/\1/p" | head -1
}

identity=${ENCORE_SIGN_IDENTITY:-}
if [ -z "$identity" ]; then
  identity=$(find_identity "Apple Development")
fi
if [ -z "$identity" ]; then
  identity=$(find_identity "Developer ID Application")
fi
if [ -z "$identity" ]; then
  identity=-
  echo "No signing certificate found; signing ad hoc (Keychain will ask again after each update)." >&2
fi

# Nested code first: the app's signature covers what's inside it.
for lib in "$app"/Contents/Frameworks/*.dylib; do
  [ -e "$lib" ] && codesign --force --sign "$identity" "$lib"
done
codesign --force --sign "$identity" --identifier "$identifier" "$app"
codesign --verify --verbose=1 "$app"
xattr -dr com.apple.quarantine "$app" 2>/dev/null || true

if [ "$identity" = - ]; then
  echo "Signed $app ad hoc"
else
  echo "Signed $app with $identity"
fi
