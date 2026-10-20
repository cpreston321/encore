#!/bin/bash
# Builds Encore.app: release binary, Info.plist, icon, signed with your Apple
# Development certificate, and installs it to ~/Applications.
#
# macOS remembers "Always Allow" for Keychain access per code signature. Unsigned builds
# get a new identity every time they're compiled, so the prompt comes back after each
# build; signing with the same certificate and identifier keeps the identity stable.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release "$@"

app=target/Encore.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/encore "$app/Contents/MacOS/Encore"
cp assets/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Encore</string>
  <key>CFBundleDisplayName</key><string>Encore</string>
  <key>CFBundleExecutable</key><string>Encore</string>
  <key>CFBundleIdentifier</key><string>dev.gpui.music</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST

identity=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Apple Development:[^"]*\)".*/\1/p' | head -1)
if [ -n "$identity" ]; then
  codesign --force --sign "$identity" --identifier dev.gpui.music "$app"
  codesign --verify --verbose=1 "$app"
else
  echo "No Apple Development certificate found; Encore.app is unsigned." >&2
fi

mkdir -p "$HOME/Applications"
rm -rf "$HOME/Applications/Encore.app"
cp -R "$app" "$HOME/Applications/Encore.app"
echo "Installed ~/Applications/Encore.app"
