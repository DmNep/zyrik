#!/bin/bash
# Сценарий проверки Зырика под X11 (контейнер из этой папки). Исходники — /src только для чтения.
# Печатает «ОК …» / «ПРОВАЛ …» по шагам и итог; код выхода 1, если что-то провалилось.
set -u
SRC=${SRC:-/src}
cd "$SRC"
fails=0
ok()   { echo "ОК     $*"; }
bad()  { echo "ПРОВАЛ $*"; fails=$((fails + 1)); }
check() { if eval "$1"; then ok "$2"; else bad "$2"; fi; }

echo "== тесты"
cargo test --locked -q 2>&1 | grep -E "test result|FAILED|panicked|error" | sort | uniq -c
cargo test --locked -q >/dev/null 2>&1 && ok "cargo test" || bad "cargo test"

echo "== сборка"
cargo build --locked --release -q 2>&1 | tail -3
exe="${CARGO_TARGET_DIR:-$SRC/target}/release/zyrik"
ls -la "$exe" | awk '{print "размер", $5}'

echo "== запуск под Xvfb"
Xvfb :99 -screen 0 1600x900x24 -nolisten tcp >/dev/null 2>&1 &
sleep 1
export DISPLAY=:99
shots=$(mktemp -d)
mkdir "$shots/Снимки"
cd /tmp
strace -f -qq -e trace=openat,creat,rename,renameat,renameat2,unlink,unlinkat,mkdir,mkdirat,connect,socket -o /tmp/strace.txt \
  "$exe" --lang ru --save-dir "$shots" 2>/tmp/zyrik.log &
zpid=$!
sleep 1.5

check '"$exe" --lang ru 2>&1 | grep -q "уже запущен"' "вторая копия не запускается"

shot() {  # Ctrl+PrtScr и выделить (200,150)–(700,450)
  xdotool key ctrl+Print; sleep 0.4
  xdotool mousemove 200 150 mousedown 1 mousemove 450 300 mousemove 700 450 mouseup 1; sleep 0.2
}

# 1. выделение → Enter → буфер
shot
check 'xdotool search --onlyvisible --name "^Zyrik$" >/dev/null' "окно выделения на экране"
xdotool key Return; sleep 0.3
targets=$(xclip -selection clipboard -t TARGETS -o 2>/dev/null)
check '[[ "$targets" == *image/png* && "$targets" == *x-kde-passwordManagerHint* ]]' "в буфере image/png и пометка для менеджеров буфера ($(echo $targets | tr '\n' ' '))"
check '[ "$(xclip -selection clipboard -t x-kde-passwordManagerHint -o 2>/dev/null)" = secret ]' "x-kde-passwordManagerHint = secret"
xclip -selection clipboard -t image/png -o > /tmp/bg.png 2>/dev/null
check '[ ! -s /tmp/bg.png ]' "без Ctrl (фоновый читатель) снимок не отдаётся"
xdotool keydown ctrl; sleep 0.05
xclip -selection clipboard -t image/png -o > /tmp/a.png 2>/dev/null
xclip -selection clipboard -t image/png -o > /tmp/b.png 2>/dev/null
xdotool keyup ctrl; sleep 0.3
size=$(python3 -c "import struct;d=open('/tmp/a.png','rb').read();print(struct.unpack('>II',d[16:24]) if d[:4]==b'\x89PNG' else 'не PNG')")
check '[ "$size" = "(500, 300)" ]' "с Ctrl вставляется PNG 500×300 ($size)"
check 'cmp -s /tmp/a.png /tmp/b.png' "пока Ctrl зажат — можно вставить ещё раз"
check '! xclip -selection clipboard -t TARGETS -o >/dev/null 2>&1' "отпустили Ctrl — буфер пуст"

# 2. Ctrl+S → файл в --save-dir
shot; xdotool key ctrl+s; sleep 0.5
n=$(ls "$shots"/*.png 2>/dev/null | wc -l)
check '[ "$n" = 1 ]' "Ctrl+S сохранил файл в --save-dir ($(ls "$shots"))"
chunks=$(python3 - "$shots" <<'EOF'
import sys,glob,struct
d=open(glob.glob(sys.argv[1]+'/*.png')[0],'rb').read();i=8;out=[]
while i+8<=len(d):
    n=struct.unpack('>I',d[i:i+4])[0];out.append(d[i+4:i+8].decode());i+=12+n
print(','.join(out))
EOF
)
check '[ "$chunks" = "IHDR,IDAT,IEND" ]' "в PNG только IHDR,IDAT,IEND ($chunks)"

# 3. дискета → выбор папки → Enter (в «Снимки») → Ctrl+S
shot
# панель под зоной справа: правый край 700, дискета — 9-я кнопка (см. toolbar.rs)
bar_w=$((4*2 + 32*11 + 10*3)); fx=$((700 - bar_w + 4 + 32*8 + 10*2 + 16)); fy=$((450 + 8 + 4 + 16))
xdotool mousemove $fx $fy click 1; sleep 0.3
check 'xdotool search --onlyvisible --name "^Zyrik$" >/dev/null' "после дискеты выделение на месте (открыт выбор папки)"
xdotool key Return; sleep 0.2; xdotool key ctrl+s; sleep 0.5
check '[ "$(ls "$shots/Снимки" | wc -l)" = 1 ]' "выбор папки: сохранено в «Снимки»"
shot; xdotool key ctrl+s; sleep 0.5
check '[ "$(ls "$shots/Снимки" | wc -l)" = 2 ]' "следующий Ctrl+S — в выбранную папку"

# 4. подпись по-русски: раскладка us,ru, вторая группа
setxkbmap -layout us,ru 2>/dev/null
shot; xdotool key t; xdotool mousemove 260 200 click 1; sleep 0.1
xdotool key --clearmodifiers ISO_Next_Group 2>/dev/null
xdotool type --delay 30 "Тест"; xdotool key Return; sleep 0.1
xdotool key Escape; sleep 0.2
check '! xdotool search --onlyvisible --name "^Zyrik$" >/dev/null' "Esc закрыл выделение"

# 5. правый щелчок без зоны — отмена
shot; xdotool click 3; sleep 0.1; xdotool click 3; sleep 0.2
check '! xdotool search --onlyvisible --name "^Zyrik$" >/dev/null' "правая кнопка: сбросить зону, потом закрыть"

pkill -x zyrik; wait $zpid 2>/dev/null

echo "== strace: что Зырик открывал на запись и куда подключался"
grep -E 'O_WRONLY|O_RDWR|O_CREAT|creat\(|rename|unlink|mkdir' /tmp/strace.txt | grep -v ENOENT | grep -vE '"/dev/(null|tty|pts)' | sed 's/^[0-9]* //' > /tmp/writes.txt
cat /tmp/writes.txt
bad_writes=$(grep -v "$shots" /tmp/writes.txt | grep -v '^$' || true)
check '[ -z "$bad_writes" ]' "на запись открывались только сохранённые снимки"
grep -E 'connect\(' /tmp/strace.txt | sed 's/^[0-9]* //' | sort | uniq -c
nonunix=$(grep -E 'connect\(' /tmp/strace.txt | grep -v 'AF_UNIX' || true)
check '[ -z "$nonunix" ]' "все соединения — только AF_UNIX"

echo "== лог Зырика"
cat /tmp/zyrik.log
echo
if [ $fails = 0 ]; then echo "ИТОГ: всё прошло"; else echo "ИТОГ: провалов $fails"; exit 1; fi
