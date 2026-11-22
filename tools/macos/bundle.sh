#!/bin/bash
# Собирает Зырик.app и zip (D7). Запускать на macOS из корня репозитория:
#   bash tools/macos/bundle.sh            → target/macos/Зырик.app и target/macos/Zyrik-macos.zip
set -euo pipefail
version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
cargo build --release --locked ${ZYRIK_FEATURES:+--features $ZYRIK_FEATURES}
out=target/macos
app="$out/Зырик.app"
rm -rf "$out" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/zyrik "$app/Contents/MacOS/zyrik"

# значок: PNG 256×256 из assets/zyrik.ico → .icns
python3 - <<'EOF'
import struct
d = open('assets/zyrik.ico', 'rb').read()
for i in range(struct.unpack('<H', d[4:6])[0]):
    size, off = struct.unpack('<II', d[6 + 16 * i + 8:6 + 16 * i + 16])
    if d[off:off + 4] == b'\x89PNG':
        open('target/macos/icon256.png', 'wb').write(d[off:off + size])
EOF
iconset="$out/zyrik.iconset"; mkdir -p "$iconset"
for s in 16 32 128 256; do
  sips -z $s $s "$out/icon256.png" --out "$iconset/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2)); [ $d -le 256 ] && sips -z $d $d "$out/icon256.png" --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/zyrik.icns"

cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.github.dmnep.zyrik</string>
  <key>CFBundleName</key><string>Zyrik</string>
  <key>CFBundleDisplayName</key><string>Зырик</string>
  <key>CFBundleExecutable</key><string>zyrik</string>
  <key>CFBundleIconFile</key><string>zyrik</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSScreenCaptureUsageDescription</key>
  <string>Зырик снимает экран только по вашей горячей клавише; снимок остаётся в памяти, пока вы его не вставите или не сохраните.</string>
</dict>
</plist>
EOF

# подпись «ad-hoc»: без неё Apple Silicon не запустит; разрешение «Запись экрана» привязано к подписи
codesign --force --deep --sign - "$app"
(cd "$out" && ditto -c -k --keepParent "Зырик.app" Zyrik-macos.zip)
echo "$app"
ls -la "$out/Zyrik-macos.zip"
