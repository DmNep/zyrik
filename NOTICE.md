# Сторонние компоненты

Что встроено сейчас — ниже. По мере добавления здесь появятся и другие лицензии:

- Tesseract (Apache-2.0) и Leptonica (BSD-2-Clause) — распознавание текста, фаза 6;
- модели `tessdata_fast` `eng`, `rus` (Apache-2.0);

Значок «о_О» нарисован скриптом `tools/make_icon.py` и распространяется под лицензией проекта (MIT).

## DejaVu Sans Bold — `assets/DejaVuSans-Bold-zyrik.ttf`

Шрифт подписей и подсказок, урезанный до нужных символов (`tools/subset_font.py`).
Fonts are (c) Bitstream; DejaVu changes are in public domain. Лицензия Bitstream Vera Fonts
разрешает распространять шрифт, в том числе в составе программ. Полный текст — `assets/LICENSE-DejaVu.txt`.

## Словарь BIP-39 — `assets/bip39-english.txt`

Английский список из 2048 слов для поиска сид-фраз, из репозитория bitcoin/bips (`bip-0039/english.txt`,
SHA-256 `2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda`). Распространяется вместе с BIP-0039
под лицензией MIT: Copyright (c) Marek Palatinus, Pavol Rusnak, Aaron Voisine, Sean Bowe.
