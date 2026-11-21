#!/bin/bash
# Дымовая проверка Зырик.app на машине CI (macOS 14+). Печатает ОК/ПРОВАЛ/НЕ ПРОВЕРЕНО.
# Захват экрана в CI зависит от разрешения «Запись экрана» — его выдаём через TCC.db, если можно.
set -u
app="target/macos/Зырик.app"
fails=0
ok()  { echo "ОК     $*"; }
bad() { echo "ПРОВАЛ $*"; fails=$((fails + 1)); }
na()  { echo "НЕ ПРОВЕРЕНО $*"; }

"$app/Contents/MacOS/zyrik" --help --lang ru | grep -q -- --save-dir && ok "--help" || bad "--help"

# попытка выдать «Запись экрана» подписи пакета (машина CI одноразовая)
req=$(codesign -d -r- "$app" 2>&1 | sed -n 's/^designated => //p')
for db in "$HOME/Library/Application Support/com.apple.TCC/TCC.db" "/Library/Application Support/com.apple.TCC/TCC.db"; do
  sudo sqlite3 "$db" "INSERT OR REPLACE INTO access (service, client, client_type, auth_value, auth_reason, auth_version, indirect_object_identifier, flags) VALUES ('kTCCServiceScreenCapture', 'com.github.dmnep.zyrik', 0, 2, 4, 1, 'UNUSED', 0);" 2>/dev/null \
    && echo "TCC: записано в $db" || echo "TCC: нельзя записать в $db"
done

log=$(mktemp)
shots=$(mktemp -d)
"$app/Contents/MacOS/zyrik" --lang ru --save-dir "$shots" 2>"$log" &
pid=$!
sleep 3
kill -0 $pid 2>/dev/null && ok "Зырик запущен и работает" || bad "Зырик завершился: $(cat "$log")"

# горячая клавиша Cmd+Shift+2 (нужен доступ System Events к клавиатуре — на машинах CI обычно есть)
if osascript -e 'tell application "System Events" to key code 19 using {command down, shift down}' 2>/dev/null; then
  sleep 1.5
  if grep -q "снято за" "$log"; then
    ok "захват экрана: $(grep 'снято за' "$log" | head -1)"
    # выделить и сохранить: cliclick тянет мышью, Cmd+S сохраняет
    if command -v cliclick >/dev/null || brew install cliclick >/dev/null 2>&1; then
      cliclick dd:200,200 m:400,300 du:600,400; sleep 0.3
      osascript -e 'tell application "System Events" to keystroke "s" using {command down}'; sleep 1
      n=$(ls "$shots" | wc -l | tr -d ' ')
      [ "$n" = 1 ] && ok "Cmd+S сохранил $(ls "$shots")" || bad "Cmd+S: файлов $n; лог: $(tail -3 "$log")"
      osascript -e 'tell application "System Events" to key code 19 using {command down, shift down}'; sleep 1.5
      cliclick dd:200,200 m:400,300 du:600,400; sleep 0.3
      osascript -e 'tell application "System Events" to key code 36'; sleep 0.5
      # типы — без запроса данных (это не вставка); данные — с зажатым Cmd, как при Cmd+V
      types() { osascript -l JavaScript -e 'ObjC.import("AppKit"); var t=$.NSPasteboard.generalPasteboard.types; t.isNil()?"":ObjC.deepUnwrap(t).join(",")' 2>&1; }
      png() { osascript -l JavaScript -e 'ObjC.import("AppKit"); var d=$.NSPasteboard.generalPasteboard.dataForType("public.png"); d.isNil()?0:d.length' 2>&1; }
      t=$(types); echo "типы в буфере: $t"
      [[ "$t" == *public.png* && "$t" == *org.nspasteboard.ConcealedType* && "$t" == *org.nspasteboard.TransientType* ]]         && ok "в буфере обещан PNG с пометками Transient и Concealed" || bad "типы в буфере: $t"
      n=$(png); [ "$n" = 0 ] && ok "без Cmd (фоновый читатель) PNG не отдаётся" || bad "без Cmd отдано $n байт"
      cliclick kd:cmd; sleep 0.2
      echo "флаги модификаторов, которые видит другой процесс: $(osascript -l JavaScript -e 'ObjC.import("AppKit"); $.NSEvent.modifierFlags' 2>&1)"
      n=$(png); echo "с Cmd получено байт: $n"
      cliclick ku:cmd; sleep 1
      [ "${n:-0}" -gt 1000 ] 2>/dev/null && ok "с Cmd PNG отдаётся ($n байт)" || bad "с Cmd PNG не отдан ($n)"
      t=$(types); [[ "$t" != *public.png* ]] && ok "отпустили Cmd — буфер очищен" || bad "после отпускания Cmd в буфере всё ещё: $t"
      # тот же опыт на свежем снимке: первый же запрос — с Cmd
      osascript -e 'tell application "System Events" to key code 19 using {command down, shift down}'; sleep 1.5
      cliclick dd:200,200 m:400,300 du:600,400; sleep 0.3
      osascript -e 'tell application "System Events" to key code 36'; sleep 0.5
      cliclick kd:cmd; sleep 0.2; n=$(png); cliclick ku:cmd; sleep 1
      echo "свежий снимок, первый запрос с Cmd: $n байт; типы после отпускания: $(types)"
    else
      na "выделение мышью: нет cliclick"
    fi
  elif grep -q "разрешение на запись экрана" "$log"; then
    na "захват: нет разрешения «Запись экрана» на машине CI"
  else
    na "захват: горячая клавиша не дошла (лог: $(tail -2 "$log"))"
  fi
else
  na "горячая клавиша: osascript не может нажимать клавиши"
fi

kill $pid 2>/dev/null
echo "--- лог"; cat "$log"
[ $fails = 0 ]
