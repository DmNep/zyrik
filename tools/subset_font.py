"""Урезает DejaVu Sans Bold до нужных Зырику символов → assets/DejaVuSans-Bold-zyrik.ttf.

Источник — любой DejaVuSans-Bold.ttf (например, из matplotlib). Нужен fontTools.
Запуск: python tools/subset_font.py <путь к DejaVuSans-Bold.ttf>
"""
import sys
from pathlib import Path

from fontTools import subset

UNICODES = [
    *range(0x20, 0x7F),      # латиница, цифры, знаки
    *range(0xA0, 0x100),     # Latin-1: «», ©, °, ×, ±, ё в составе не нужен, но дёшево
    *range(0x400, 0x460),    # кириллица
    *range(0x2010, 0x2028),  # тире, кавычки, …
    *range(0x2030, 0x205F),  # ‰, ′, ″ и пр.
    0x20AC, 0x20BD, 0x2116,  # € ₽ №
    *range(0x2190, 0x2196),  # стрелки ← ↑ → ↓ ↔ ↕
    0x2713, 0x2715, 0x2716,  # ✓ ✕ ✖
]


def main():
    src = Path(sys.argv[1])
    out = Path(__file__).resolve().parent.parent / "assets" / "DejaVuSans-Bold-zyrik.ttf"
    opts = subset.Options()
    opts.layout_features = ["kern"]
    opts.name_IDs = ["*"]  # сохраняем копирайт и лицензию внутри шрифта
    opts.hinting = False
    opts.notdef_outline = True
    font = subset.load_font(str(src), opts)
    sub = subset.Subsetter(opts)
    sub.populate(unicodes=UNICODES)
    sub.subset(font)
    subset.save_font(font, str(out), opts)
    print(out, out.stat().st_size)


if __name__ == "__main__":
    main()
