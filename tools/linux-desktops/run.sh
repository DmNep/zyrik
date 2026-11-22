#!/bin/bash
# Зырик на настоящем рабочем столе (X11) в контейнере: kde | xfce | gnome.
# Снимки экрана по шагам — в /out/<стол>/, итог — ОК/ПРОВАЛ/НЕ ПРОВЕРЕНО.
set -u
de=${1:-kde}
out=/out/$de
mkdir -p "$out" "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
fails=0
ok()   { echo "ОК     $*"; }
bad()  { echo "ПРОВАЛ $*"; fails=$((fails + 1)); }
na()   { echo "НЕ ПРОВЕРЕНО $*"; }
shot_png() { import -window root "$out/$1.png" 2>/dev/null; }

Xvfb :99 -screen 0 1600x900x24 +extension GLX -nolisten tcp >/dev/null 2>&1 &
sleep 1
export DISPLAY=:99
eval "$(dbus-launch --sh-syntax)"
echo "D-Bus: $DBUS_SESSION_BUS_ADDRESS"

case $de in
  kde)
    kded5 >/tmp/kded.log 2>&1 &   # в нём StatusNotifierWatcher — без него трей не видит значки
    kwin_x11 >/tmp/wm.log 2>&1 &
    sleep 2
    plasmashell >/tmp/shell.log 2>&1 &
    sleep 3
    # в полном сеансе (startplasma) модуль загружается сам; здесь сеанс собран вручную
    qdbus org.kde.kded5 /kded org.kde.kded5.loadModule statusnotifierwatcher >/dev/null
    ;;
  xfce)
    mkdir -p ~/.config/xfce4/xfconf/xfce-perchannel-xml
    cp /etc/xdg/xfce4/panel/default.xml ~/.config/xfce4/xfconf/xfce-perchannel-xml/xfce4-panel.xml
    xfwm4 >/tmp/wm.log 2>&1 &
    sleep 1
    xfce4-panel >/tmp/shell.log 2>&1 &
    ;;
  gnome)
    gsettings set org.gnome.shell enabled-extensions "['ubuntu-appindicators@ubuntu.com']"
    gsettings set org.gnome.shell disable-user-extensions false
    gnome-shell --x11 --replace >/tmp/shell.log 2>&1 &
    ;;
esac
sleep 10
# GNOME после входа открывает обзор (Overview) и держит в нём клавиатуру — закрываем, как сделал бы человек
[ "$de" = gnome ] && xdotool key Escape && sleep 1
shot_png 0-desktop
pgrep -l "plasmashell|xfce4-panel|gnome-shell" >/dev/null && ok "рабочий стол $de запущен" || bad "рабочий стол $de не запустился: $(tail -3 /tmp/shell.log)"

shots=$(mktemp -d); mkdir "$shots/Снимки"
"${ZYRIK:-/bin-zyrik/zyrik}" --lang ru --save-dir "$shots" 2>/tmp/z.log &
zpid=$!
sleep 4
kill -0 $zpid 2>/dev/null && ok "Зырик работает" || bad "Зырик завершился: $(cat /tmp/z.log)"

# --- значок в трее (StatusNotifierItem)
items=$(dbus-send --session --print-reply --dest=org.kde.StatusNotifierWatcher /StatusNotifierWatcher \
  org.freedesktop.DBus.Properties.Get string:org.kde.StatusNotifierWatcher string:RegisteredStatusNotifierItems 2>&1)
svc=$(echo "$items" | grep -o '"[^"]*StatusNotifierItem[^"]*"' | tr -d '"' | grep -i "$(pgrep -x zyrik)\|zyrik" | head -1)
[ -z "$svc" ] && svc=$(echo "$items" | grep -o '"[^"]*"' | tr -d '"' | grep -v '^$' | head -1)
if [ -n "$svc" ]; then ok "значок зарегистрирован в трее: $svc"; else bad "значка нет в трее: $items"; fi
shot_png 1-tray
dest=${svc%%/*}

# --- горячая клавиша Ctrl+PrtScr
n0=$(grep -c "снято за" /tmp/z.log)
xdotool key ctrl+Print; sleep 1
if [ "$(grep -c "снято за" /tmp/z.log)" -gt "$n0" ]; then ok "Ctrl+PrtScr открыл выделение"; hot=1
else hot=0
  if grep -q "уже заняла" /tmp/z.log; then na "Ctrl+PrtScr занята рабочим столом — Зырик об этом сообщил: $(grep -m1 'заняла' /tmp/z.log)"
  else bad "Ctrl+PrtScr: ничего не произошло"; fi
fi
[ $hot = 1 ] && xdotool key Escape && sleep 0.3

# --- щелчок по значку (Activate) → «Зырнуть»
start() {
  if [ -n "$dest" ]; then
    dbus-send --session --print-reply --dest="$dest" /StatusNotifierItem org.kde.StatusNotifierItem.Activate int32:0 int32:0 2>&1 | grep -v "^method return"
  else
    xdotool key ctrl+Print
  fi
  sleep 1.5
}
n0=$(grep -c "снято за" /tmp/z.log)
start
if [ -z "$dest" ]; then na "щелчок по значку: значка нет, выделение открыто горячей клавишей"
elif [ "$(grep -c "снято за" /tmp/z.log)" -gt "$n0" ]; then ok "щелчок по значку открыл выделение"
else bad "щелчок по значку: выделение не открылось"; fi
xdotool mousemove 300 200 mousedown 1 mousemove 600 400 mousemove 900 600 mouseup 1; sleep 0.4
shot_png 2-selection
xdotool key a; xdotool mousemove 850 550 mousedown 1 mousemove 500 300 mouseup 1; sleep 0.2
shot_png 3-arrow
xdotool key Return; sleep 0.5

# --- буфер: менеджер буфера (Klipper в KDE) не должен сохранить снимок
targets=$(xclip -selection clipboard -t TARGETS -o 2>/dev/null | tr '\n' ' ')
[[ "$targets" == *image/png* ]] && ok "снимок в буфере ($targets)" || bad "в буфере нет снимка ($targets)"
xdotool keydown ctrl; sleep 0.05
xclip -selection clipboard -t image/png -o > /tmp/a.png 2>/dev/null
xdotool keyup ctrl; sleep 1
size=$(python3 -c "import struct;d=open('/tmp/a.png','rb').read();print(struct.unpack('>II',d[16:24]) if d[:4]==b'\x89PNG' else 'не PNG')")
[ "$size" = "(600, 400)" ] && ok "Ctrl+V вставил PNG 600×400" || bad "вставка: $size"
after=$(xclip -selection clipboard -t TARGETS -o 2>/dev/null | tr '\n' ' ')
[[ "$after" != *image/png* ]] && ok "после вставки снимка в буфере нет и менеджер буфера его не вернул (сейчас: ${after:-пусто})" \
  || bad "после вставки в буфере снова картинка — её сохранил менеджер буфера ($after)"
if [ "$de" = kde ]; then
  hist=$(qdbus org.kde.klipper /klipper org.kde.klipper.klipper.getClipboardHistoryMenu 2>&1 | head -5)
  echo "история Klipper: ${hist:-пусто}"
fi

# --- Ctrl+S и выбор папки (дискета)
start; xdotool mousemove 300 200 mousedown 1 mousemove 900 600 mouseup 1; sleep 0.3
xdotool key ctrl+s; sleep 0.6
[ "$(ls "$shots"/*.png 2>/dev/null | wc -l)" = 1 ] && ok "Ctrl+S сохранил файл" || bad "Ctrl+S: $(ls "$shots")"
start; xdotool mousemove 300 200 mousedown 1 mousemove 900 600 mouseup 1; sleep 0.3
bar_w=$((4*2 + 32*14 + 10*4)); fx=$((900 - bar_w + 4 + 32*11 + 10*3 + 16)); fy=$((600 + 8 + 4 + 16))
xdotool mousemove $fx $fy click 1; sleep 0.5
shot_png 4-picker
xdotool key Return; sleep 0.2; xdotool key ctrl+s; sleep 0.6
[ "$(ls "$shots/Снимки" | wc -l)" = 1 ] && ok "выбор папки: сохранено в «Снимки»" || bad "выбор папки: $(ls -R "$shots")"

# --- пункт меню «Выход» через меню значка (dbusmenu)
if [ -n "$dest" ]; then
  layout=$(dbus-send --session --print-reply --dest="$dest" /MenuBar com.canonical.dbusmenu.GetLayout int32:0 int32:-1 array:string: 2>&1)
  exit_id=$(echo "$layout" | python3 -c "
import sys,re
t=sys.stdin.read()
ids=[int(m) for m in re.findall(r'struct \{\s*int32 (\d+)', t)]
labels=re.findall(r'string \"label\"\s*variant\s*string \"([^\"]*)\"', t)
pairs=list(zip(ids[1:], labels))
print(next((i for i,l in pairs if 'Выход' in l), ''))
print(pairs, file=sys.stderr)")
  echo "меню: id «Выход» = $exit_id"
  if [ -n "$exit_id" ]; then
    dbus-send --session --print-reply --dest="$dest" /MenuBar com.canonical.dbusmenu.Event int32:$exit_id string:clicked variant:int32:0 uint32:0 2>&1 | sed "s/^/  ответ: /" | head -2
    for _ in 1 2 3 4 5 6; do kill -0 $zpid 2>/dev/null || break; sleep 0.5; done
    kill -0 $zpid 2>/dev/null && for t in /proc/$zpid/task/*; do echo "  поток $(cat $t/comm): $(cat $t/wchan)"; done
    kill -0 $zpid 2>/dev/null && bad "«Выход» в меню не завершил Зырик" || ok "«Выход» в меню значка завершил Зырик"
  else bad "в меню нет «Выход»"; fi
fi
kill $zpid 2>/dev/null
echo "--- лог Зырика"; cat /tmp/z.log
echo "ИТОГ $de: провалов $fails"
[ $fails = 0 ]
