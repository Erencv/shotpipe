#!/bin/bash
# Build Shotpipe.app — a proper macOS menu bar app bundle with the icon.
# Usage: scripts/make-app.sh   → ./Shotpipe.app (drag into /Applications)
set -euo pipefail
cd "$(dirname "$0")/.."

# SHOTPIPE_BIN lets CI pass a prebuilt (e.g. universal) binary; default: build here.
BIN="${SHOTPIPE_BIN:-}"
if [[ -z "$BIN" ]]; then
  cargo build --release
  BIN=target/release/shotpipe
fi

APP="Shotpipe.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/Shotpipe"
cp assets/Shotpipe.icns "$APP/Contents/Resources/Shotpipe.icns"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Shotpipe</string>
    <key>CFBundleDisplayName</key><string>Shotpipe</string>
    <key>CFBundleIdentifier</key><string>com.erencavus.shotpipe</string>
    <key>CFBundleVersion</key><string>0.1.0</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>Shotpipe</string>
    <key>CFBundleIconFile</key><string>Shotpipe</string>
    <key>LSUIElement</key><true/>
    <key>LSMinimumSystemVersion</key><string>12.0</string>
    <key>NSDescription</key><string>Seamless screenshots from your Mac to your dev servers.</string>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true

echo "Built $APP — drag it into /Applications to install."
echo "Login item tip: run it once from /Applications, then enable 'Start at login' in its menu."
