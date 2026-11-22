#!/bin/bash
# Статическая сборка Leptonica + Tesseract для встроенного распознавания (план D4, R4).
# Всё, что открывает файлы или сеть, выключено: форматы картинок, curl, libarchive, графика.
#   bash tools/ocr-build.sh [папка установки]   (по умолчанию target/ocr/install)
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
prefix=${1:-$root/target/ocr/install}
build=$(dirname "$prefix"); mkdir -p "$build" "$prefix"
gen=(-G Ninja)
common=(-DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DSW_BUILD=OFF -DCMAKE_INSTALL_PREFIX="$prefix"
        -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DCMAKE_OSX_DEPLOYMENT_TARGET=14.0)
cmake -S "$root/third_party/leptonica" -B "$build/lept" "${gen[@]}" "${common[@]}" \
  -DENABLE_ZLIB=OFF -DENABLE_PNG=OFF -DENABLE_GIF=OFF -DENABLE_JPEG=OFF -DENABLE_TIFF=OFF \
  -DENABLE_WEBP=OFF -DENABLE_OPENJPEG=OFF -DBUILD_PROG=OFF >"$build/lept-conf.log"
cmake --build "$build/lept" -j >"$build/lept-build.log"
cmake --install "$build/lept" >/dev/null
cmake -S "$root/third_party/tesseract" -B "$build/tess" "${gen[@]}" "${common[@]}" \
  -DLeptonica_DIR="$prefix/lib/cmake/leptonica" \
  -DBUILD_TRAINING_TOOLS=OFF -DBUILD_TESTS=OFF -DDISABLE_ARCHIVE=ON -DDISABLE_CURL=ON -DDISABLE_TIFF=ON \
  -DGRAPHICS_DISABLED=ON -DOPENMP_BUILD=OFF -DDISABLED_LEGACY_ENGINE=ON -DENABLE_NATIVE=OFF \
  -DINSTALL_CONFIGS=OFF -DENABLE_CCACHE=OFF -DENABLE_PRECOMPILED_HEADERS=OFF >"$build/tess-conf.log"
cmake --build "$build/tess" -j --target libtesseract >"$build/tess-build.log"
cmake --install "$build/tess" >/dev/null 2>&1 || cp "$build"/tess/libtesseract*.a "$prefix/lib/"
ls -la "$prefix/lib"/*.a "$prefix/lib"/*leptonica* 2>/dev/null
