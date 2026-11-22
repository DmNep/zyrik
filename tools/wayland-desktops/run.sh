#!/bin/bash
# Зырик на KDE Plasma 6 (KWin) и GNOME (Mutter) под Wayland, без экрана — в контейнере из этой папки.
#   docker build -t zyrik-wl-desktops tools/wayland-desktops
#   docker run --rm --cap-add SYS_NICE -v <папка с zyrik-debug, kwin_input, mutter_input>:/b:ro \
#     --entrypoint bash zyrik-wl-desktops /src/tools/wayland-desktops/run.sh kde|gnome
# Нужна ОТЛАДОЧНАЯ сборка Зырика (zyrik-debug): KWin и Mutter без GPU снимать экран не умеют
# (KWin падает в screenshot-плагине на RenderDevice::eglContext, у Mutter без gnome-shell нет портала),
# поэтому кадр подставляется из файла (ZYRIK_TEST_FRAME, только в отладочной сборке).
# Проверяется всё остальное: окно выделения, мышь, клавиши, буфер, Ctrl+S.
set -u
de=${1:-kde}
B=${ZYRIK_BIN:-/b}
mkdir -p "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
fails=0
ok()  { echo "ОК     $*"; }
bad() { echo "ПРОВАЛ $*"; fails=$((fails + 1)); }

python3 - <<'EOF'
import zlib, struct
w, h = 1600, 900
rows = bytearray()
for y in range(h):
    rows.append(0)
    for x in range(w):
        rows += bytes(((x * 255) // w, (y * 255) // h, 160))
def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
open("/tmp/frame.png", "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
    + chunk(b"IDAT", zlib.compress(bytes(rows))) + chunk(b"IEND", b""))
EOF

case $de in
  kde)
    export XDG_CURRENT_DESKTOP=KDE QT_QPA_PLATFORM=wayland WAYLAND_DISPLAY=wl-kde LIBGL_ALWAYS_SOFTWARE=1
    eval "$(dbus-launch --sh-syntax)"
    kwin_wayland --virtual --width 1600 --height 900 --no-lockscreen --socket wl-kde >/tmp/wm.log 2>&1 &
    sleep 8
    INPUT=$B/kwin_input
    ;;
  gnome)
    eval "$(dbus-launch --sh-syntax)"
    mutter --headless --wayland --no-x11 --virtual-monitor 1600x900 --wayland-display wl-gn >/tmp/wm.log 2>&1 &
    sleep 6
    export WAYLAND_DISPLAY=wl-gn
    INPUT=$B/mutter_input
    ;;
esac
mkfifo /tmp/in; "$INPUT" < /tmp/in 2>/tmp/in.log & exec 3>/tmp/in
sleep 1; echo "key 42" >&3; sleep 0.5   # виртуальная клавиатура появляется с первым нажатием
act() { echo "$*" >&3; sleep 1.2; }

shots=$(mktemp -d)
ZYRIK_TEST_FRAME=/tmp/frame.png "$B/zyrik-debug" --lang ru --save-dir "$shots" 2>/tmp/z.log &
sleep 2
"$B/zyrik-debug" --trigger; sleep 1.5
grep -q "снято за" /tmp/z.log && ok "$de: выделение открыто" || bad "$de: выделение не открылось: $(tail -2 /tmp/z.log)"
act drag 300 200 900 600; act key 28
grep -q "зырнул: область 600x400" /tmp/z.log && ok "$de: мышь и Enter — снимок 600×400" || bad "$de: Enter: $(tail -2 /tmp/z.log)"
sleep 0.5
t=$(timeout 3 wl-paste --list-types 2>&1 | tr '\n' ' ')
[[ "$t" == *image/png* && "$t" == *x-kde-passwordManagerHint* ]] && ok "$de: в буфере image/png и пометка ($t)" || bad "$de: типы: $t"
n=$(timeout 3 wl-paste -t image/png 2>/dev/null | wc -c)
[ "$n" -gt 100 ] && ok "$de: вставка — $n байт" || bad "$de: вставка пустая"
sleep 2.5
n=$(timeout 3 wl-paste -t image/png 2>/dev/null | wc -c)
[ "$n" = 0 ] && ok "$de: через 1,5 с после вставки снимка в буфере нет" || bad "$de: снимок остался в буфере ($n байт)"
"$B/zyrik-debug" --trigger; sleep 1.5; act drag 300 200 900 600; act key ctrl+31
[ "$(ls "$shots"/*.png 2>/dev/null | wc -l)" = 1 ] && ok "$de: Ctrl+S сохранил" || bad "$de: Ctrl+S: $(ls "$shots")"
"$B/zyrik-debug" --trigger; sleep 1.5; act key 1
m=$(grep -c "снято за" /tmp/z.log); "$B/zyrik-debug" --trigger; sleep 1.5
[ "$(grep -c "снято за" /tmp/z.log)" -gt "$m" ] && ok "$de: Esc закрыл, следующее открылось" || bad "$de: после Esc не открылось"
echo "--- лог"; cat /tmp/z.log
echo "ИТОГ $de: провалов $fails"
[ $fails = 0 ]
