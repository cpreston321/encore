#!/bin/bash
# Builds Encore.app: release binary, Info.plist, icon, signed with your certificate (see
# scripts/sign.sh; ad hoc without one), and installs it to ~/Applications.
set -euo pipefail
cd "$(dirname "$0")/.."

# DIST=1 builds the smaller, slower-to-compile `dist` profile for builds you ship.
profile=release
[ "${DIST:-}" = 1 ] && profile=dist
cargo build --profile "$profile" "$@"

app=target/Encore.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
mkdir -p "$app/Contents/Frameworks"
cp "target/$profile/encore" "$app/Contents/MacOS/Encore"
# The native MusicKit player; without it, full songs use the web player.
if [ -f "target/$profile/libencore_native.dylib" ]; then
  cp "target/$profile/libencore_native.dylib" "$app/Contents/Frameworks/"
fi
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
  <key>CFBundleIdentifier</key><string>io.github.cpreston321.encore</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSAppleMusicUsageDescription</key><string>Encore plays songs from your Apple Music subscription.</string>
  <key>NSAudioCaptureUsageDescription</key><string>Encore sets the volume of the songs it plays by routing their audio through itself. Nothing is recorded.</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST

./scripts/sign.sh "$app"

# ENCORE_NO_INSTALL=1 leaves the app in target/, for packaging a release.
[ "${ENCORE_NO_INSTALL:-}" = 1 ] && exit 0
mkdir -p "$HOME/Applications"
rm -rf "$HOME/Applications/Encore.app"
cp -R "$app" "$HOME/Applications/Encore.app"
echo "Installed ~/Applications/Encore.app"
