#!/bin/bash
# SC-007: Зырик в простое — ≈0% CPU и ≤ 50 МБ памяти. Запускает программу, ждёт, пока
# она устроится, и 20 с смотрит, сколько процессорного времени она потратила и сколько
# памяти занимает. Использование: bash tools/idle.sh <путь к zyrik>
set -u
exe=$1
secs=20
limit_mb=50

if [ "${OS:-}" = Windows_NT ]; then
  powershell -NoProfile -Command "
    \$p = Start-Process -FilePath '$(cygpath -w "$exe")' -ArgumentList '--lang','en' -PassThru
    Start-Sleep 5
    \$p.Refresh(); \$c0 = \$p.TotalProcessorTime.TotalSeconds
    Start-Sleep $secs
    \$p.Refresh(); \$c1 = \$p.TotalProcessorTime.TotalSeconds
    \$ws = [math]::Round(\$p.WorkingSet64 / 1MB, 1); \$priv = [math]::Round(\$p.PrivateMemorySize64 / 1MB, 1)
    Stop-Process -Id \$p.Id
    'cpu_s=' + [math]::Round(\$c1 - \$c0, 3) + ' rss_mb=' + \$ws + ' private_mb=' + \$priv
  " | tr -d '\r' > /tmp/idle.txt
else
  "$exe" --lang en 2>/dev/null &
  pid=$!
  sleep 5
  cpu() {  # процессорное время в секундах
    if [ -r /proc/$pid/stat ]; then
      awk -v hz="$(getconf CLK_TCK)" '{printf "%.3f", ($14 + $15) / hz}' /proc/$pid/stat
    else  # macOS: ps -o time= → м:сс.сс
      ps -o time= -p $pid | awk -F: '{printf "%.3f", $1 * 60 + $2}'
    fi
  }
  c0=$(cpu); sleep $secs; c1=$(cpu)
  rss=$(ps -o rss= -p $pid | awk '{printf "%.1f", $1 / 1024}')
  kill $pid
  echo "cpu_s=$(awk -v a="$c0" -v b="$c1" 'BEGIN{printf "%.3f", b - a}') rss_mb=$rss" > /tmp/idle.txt
fi

cat /tmp/idle.txt
eval "$(cat /tmp/idle.txt)"
pct=$(awk -v c="$cpu_s" -v s=$secs 'BEGIN{printf "%.2f", 100 * c / s}')
echo "простой $secs с: CPU ${pct}%, память ${rss_mb} МБ (норма: ≈0% и ≤ $limit_mb МБ)"
awk -v p="$pct" -v m="$rss_mb" -v l=$limit_mb 'BEGIN{exit !(p < 0.5 && m <= l)}' && echo "ОК SC-007" || { echo "ПРОВАЛ SC-007"; exit 1; }
