#!/bin/bash
# T072: распознавание под strace — ни одного открытия файла (кроме загрузки системных библиотек
# при запуске) и ни одного сокета. Нужна собранная фича ocr (tools/ocr-build.sh).
#   ZYRIK_OCR_LIB=… bash tools/ocr-no-disk.sh
set -euo pipefail
cd "$(dirname "$0")/.."
bin=$(cargo test --release --locked --features ocr --test ocr --no-run --color never 2>&1 | sed -n 's/.*Executable tests\/ocr.rs (\(.*\))/\1/p')
[ -x "$bin" ] || { echo "не нашёл тест распознавания"; exit 1; }
log=$(mktemp)
strace -f -qq -e trace=%file,%network -o "$log" "$bin" --test-threads=1
# разрешено: запуск, загрузка системных библиотек, /proc/self/maps (рантайм Rust) и
# /proc/sys/vm/overcommit_memory (malloc в glibc ≥ 2.39) — только чтение настроек ядра
extra=$(sed 's/^[0-9]* *//' "$log" \
  | grep -vE '^(execve|arch_prctl)\(' \
  | grep -vE '"/(lib|lib64|usr/lib|etc/ld\.so\.cache)|ld\.so\.preload|newfstatat\([0-9]+, ""|"/proc/self/maps"|"/proc/sys/vm/overcommit_memory", O_RDONLY' || true)
if [ -n "$extra" ]; then
  echo "ПРОВАЛ: распознавание обращалось к файлам или сети:"; echo "$extra"; exit 1
fi
echo "ОК: распознавание не открывало файлов и сокетов"
