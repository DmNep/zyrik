#!/bin/bash
# Зырик под Wayland (Sway headless). Итог — ОК/ПРОВАЛ/НЕ ПРОВЕРЕНО по шагам, снимки экрана — в /out/<композитор>/.
set -u
wm=${1:-sway}
bin=${ZYRIK_BIN:-/bin-zyrik}
out=/out/$wm; mkdir -p "$out" "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
fails=0
ok()  { echo "ОК     $*"; }
bad() { echo "ПРОВАЛ $*"; fails=$((fails + 1)); }
na()  { echo "НЕ ПРОВЕРЕНО $*"; }
W=1600; H=900
# одна виртуальная мышь на весь сценарий (команды через FIFO): Sway путает фокус, если
# виртуальные мыши то появляются, то исчезают, — у настоящего компьютера мышь одна
mouse() { echo "$*" >&3; sleep 0.7; }
eval "$(dbus-launch --sh-syntax)"
shots=$(mktemp -d); mkdir "$shots/Снимки"
cat > /tmp/sway.conf <<EOF
output HEADLESS-1 resolution ${W}x${H} bg #3a6ea5 solid_color
bindsym Ctrl+Print exec $bin/zyrik --trigger
exec_always true
EOF
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman WLR_HEADLESS_OUTPUTS=1
sway -c /tmp/sway.conf >/tmp/wm.log 2>&1 &
for _ in $(seq 50); do [ -n "$(ls $XDG_RUNTIME_DIR | grep -m1 '^wayland-[0-9]$')" ] && break; sleep 0.2; done
export WAYLAND_DISPLAY=$(ls $XDG_RUNTIME_DIR | grep -m1 '^wayland-[0-9]$')
[ -n "$WAYLAND_DISPLAY" ] && ok "Sway запущен ($WAYLAND_DISPLAY)" || { bad "Sway не запустился: $(tail -5 /tmp/wm.log)"; exit 1; }
# постоянные клавиатура и мышь, как у настоящего компьютера: wtype и wl_mouse на каждый вызов
# создают свои и сразу убирают — к таким не успеть подключиться
mkfifo /tmp/mouse; "$bin/wl_mouse" $W $H - < /tmp/mouse & exec 3>/tmp/mouse
(sleep 100000 | wtype -s 100000000 -k Shift_L) >/dev/null 2>&1 &
sleep 1
grim "$out/0-desktop.png"

strace -f -qq -e trace=openat,creat,rename,renameat,renameat2,unlink,unlinkat,mkdir,mkdirat,connect -o /tmp/strace.txt \
  "$bin/zyrik" --lang ru --save-dir "$shots" 2>/tmp/z.log &
sleep 2
zpid=$(pgrep -x zyrik)
[ -n "$zpid" ] && ok "Зырик работает под Wayland" || bad "Зырик не запустился: $(cat /tmp/z.log)"
grep -q "пробую X11" /tmp/z.log && bad "Зырик не взял Wayland: $(grep Wayland /tmp/z.log)"

snaps() { grep -c "снято за" /tmp/z.log; }
n0=$(snaps); "$bin/zyrik" --trigger && sleep 1
[ "$(snaps)" -gt "$n0" ] && ok "zyrik --trigger открыл выделение: $(grep 'снято за' /tmp/z.log | tail -1)" || bad "--trigger: выделение не открылось"
wtype -k Escape; sleep 0.4

n0=$(snaps); wtype -M ctrl -k Print -m ctrl; sleep 1.2
[ "$(snaps)" -gt "$n0" ] && ok "бинд Sway Ctrl+PrtScr → --trigger → выделение" || bad "бинд Sway: выделение не открылось"
mouse drag 300 200 900 600; sleep 0.3
grim "$out/1-selection.png"
wtype a; mouse drag 850 550 500 300; sleep 0.2
wtype t; mouse click 320 230; sleep 0.1; wtype "Тест Wayland"; wtype -k Return; sleep 0.2
grim "$out/2-marks.png"
wtype -k Return; sleep 0.8
grep -q "зырнул: область 600x400" /tmp/z.log && ok "Enter: снимок 600×400 в буфере" || bad "Enter: $(tail -2 /tmp/z.log)"

types=$(wl-paste --list-types 2>&1 | tr '\n' ' ')
[[ "$types" == *image/png* && "$types" == *x-kde-passwordManagerHint* ]] && ok "в буфере image/png и пометка для менеджеров ($types)" || bad "типы в буфере: $types"
[ "$(wl-paste -t x-kde-passwordManagerHint 2>/dev/null)" = secret ] && ok "x-kde-passwordManagerHint = secret" || bad "x-kde-passwordManagerHint"
sleep 0.5
wl-paste -t image/png > /tmp/a.png 2>/dev/null
size=$(python3 -c "import struct;d=open('/tmp/a.png','rb').read();print(struct.unpack('>II',d[16:24]) if d[:4]==b'\x89PNG' else 'не PNG')")
[ "$size" = "(600, 400)" ] && ok "вставка: PNG 600×400" || bad "вставка: $size"
wl-paste -t image/png > /tmp/b.png 2>/dev/null
cmp -s /tmp/a.png /tmp/b.png && ok "сразу ещё раз — тоже вставляется" || bad "повторная вставка не совпала"
sleep 2
t=$(wl-paste --list-types 2>&1 | tr '\n' ' ')
[[ "$t" != *image/png* ]] && ok "через 1,5 с после вставки буфер пуст (${t:-пусто})" || bad "буфер не очистился: $t"

# Ctrl+S и выбор папки
wtype -M ctrl -k Print -m ctrl; sleep 1; mouse drag 300 200 900 600; sleep 0.2
wtype -M ctrl s -m ctrl; sleep 0.6
[ "$(ls "$shots"/*.png 2>/dev/null | wc -l)" = 1 ] && ok "Ctrl+S сохранил файл" || bad "Ctrl+S: $(ls "$shots")"
wtype -M ctrl -k Print -m ctrl; sleep 1; mouse drag 300 200 900 600; sleep 0.2
bar_w=$((4*2 + 32*11 + 10*3)); fx=$((900 - bar_w + 4 + 32*8 + 10*2 + 16)); fy=$((600 + 8 + 4 + 16))
mouse click $fx $fy; sleep 0.4
grim "$out/3-picker.png"
wtype -k Return; sleep 0.2; wtype -M ctrl s -m ctrl; sleep 0.6
[ "$(ls "$shots/Снимки" | wc -l)" = 1 ] && ok "выбор папки: сохранено в «Снимки»" || bad "выбор папки: $(ls -R "$shots")"
wtype -M ctrl -k Print -m ctrl; sleep 1; mouse click 800 450 right; sleep 0.3
n_before=$(snaps); wtype -M ctrl -k Print -m ctrl; sleep 1
[ "$(snaps)" -gt "$n_before" ] && ok "правая кнопка закрыла выделение, следующее открывается" || bad "после правой кнопки выделение не открылось снова"
wtype -k Escape; sleep 0.3

kill $zpid 2>/dev/null; sleep 0.5
echo "== strace: запись и соединения"
grep -E 'O_WRONLY|O_RDWR|O_CREAT|creat\(|rename|unlink|mkdir' /tmp/strace.txt | grep -v ENOENT | grep -vE '"/dev/(null|tty|pts)' | sed 's/^[0-9]* //' > /tmp/writes.txt
cat /tmp/writes.txt
bad_writes=$(grep -v "$shots" /tmp/writes.txt | grep -v '^$' || true)
[ -z "$bad_writes" ] && ok "на запись открывались только сохранённые снимки" || bad "лишняя запись на диск"
grep -E 'connect\(' /tmp/strace.txt | sed 's/^[0-9]* //' | sort | uniq -c
nonunix=$(grep -E 'connect\(' /tmp/strace.txt | grep -v AF_UNIX || true)
[ -z "$nonunix" ] && ok "все соединения — только AF_UNIX" || bad "есть не-unix соединения"
echo "--- лог Зырика"; cat /tmp/z.log
echo "ИТОГ $wm: провалов $fails"
[ $fails = 0 ]
