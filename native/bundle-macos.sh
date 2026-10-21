#!/bin/sh
# Builds the native Ateliet (no web view) and wraps it as "Ateliet Native.app".
#   sh native/bundle-macos.sh            → target/release/Ateliet Native.app
#   sh native/bundle-macos.sh --install  → also copies it to /Applications
set -e
cd "$(dirname "$0")/.."
cargo build -p ateliet-native --release
APP="target/release/Ateliet Native.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/ateliet-native "$APP/Contents/MacOS/ateliet-native"
cp icons/icon.icns "$APP/Contents/Resources/icon.icns"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' native/Cargo.toml | head -1)
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Ateliet Native</string>
  <key>CFBundleDisplayName</key><string>Ateliet</string>
  <key>CFBundleIdentifier</key><string>com.ateliet.native</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>ateliet-native</string>
  <key>CFBundleIconFile</key><string>icon.icns</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
echo "built: $APP"
if [ "$1" = "--install" ]; then
  rm -rf "/Applications/Ateliet Native.app"
  cp -R "$APP" /Applications/
  echo "installed: /Applications/Ateliet Native.app"
fi
