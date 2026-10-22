#!/bin/sh
# Builds Ateliet and wraps it as "Ateliet.app".
#   sh scripts/bundle-macos.sh            → target/release/Ateliet.app
#   sh scripts/bundle-macos.sh --install  → also replaces /Applications/Ateliet.app
set -e
cd "$(dirname "$0")/.."
cargo build -p ateliet --release
APP="target/release/Ateliet.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/ateliet "$APP/Contents/MacOS/ateliet"
cp icons/icon.icns "$APP/Contents/Resources/icon.icns"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Ateliet</string>
  <key>CFBundleDisplayName</key><string>Ateliet</string>
  <key>CFBundleIdentifier</key><string>md.ateliet.app</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>ateliet</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force --deep --sign - "$APP" 2>/dev/null || true
echo "built: $APP"
if [ "$1" = "--install" ]; then
  rm -rf "/Applications/Ateliet.app"
  cp -R "$APP" /Applications/
  echo "installed: /Applications/Ateliet.app"
fi
