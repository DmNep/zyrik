# Сторонние компоненты

Что встроено сейчас — ниже. По мере добавления здесь появятся и другие лицензии:


Значок «о_О» нарисован скриптом `tools/make_icon.py` и распространяется под лицензией проекта (MIT).

## Tesseract OCR — `third_party/tesseract` (подмодуль, 5.5.4)

Apache License 2.0. Copyright (c) Google и участники проекта tesseract-ocr. Собирается статически
(`tools/ocr-build.sh`) и входит в сборки с фичей `ocr`.

## Leptonica — `third_party/leptonica` (подмодуль, 1.87.0)

Лицензия в стиле BSD 2-Clause. Copyright (c) 2001 Leptonica (Dan Bloomberg).

## Модели tessdata_fast — `assets/tessdata/{eng,rus}.traineddata`

Apache License 2.0, из репозитория tesseract-ocr/tessdata_fast. SHA-256:
`eng` 7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2,
`rus` e16e5e036cce1d9ec2b00063cf8b54472625b9e14d893a169e2b0dedeb4df225.

## DejaVu Sans Bold — `assets/DejaVuSans-Bold-zyrik.ttf`

Шрифт подписей и подсказок, урезанный до нужных символов (`tools/subset_font.py`).
Fonts are (c) Bitstream; DejaVu changes are in public domain. Лицензия Bitstream Vera Fonts
разрешает распространять шрифт, в том числе в составе программ. Полный текст — `assets/LICENSE-DejaVu.txt`.

## Словарь BIP-39 — `assets/bip39-english.txt`

Английский список из 2048 слов для поиска сид-фраз, из репозитория bitcoin/bips (`bip-0039/english.txt`,
SHA-256 `2f5eed53a4727b4bf8880d8f3f199efc90e58503646d9ff8eff3a2ed3b24dbda`). Распространяется вместе с BIP-0039
под лицензией MIT: Copyright (c) Marek Palatinus, Pavol Rusnak, Aaron Voisine, Sean Bowe.
