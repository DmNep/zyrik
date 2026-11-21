//! Текст: подписи на снимке и подсказки панели. Шрифт DejaVu Sans Bold, урезанный до латиницы,
//! кириллицы и знаков (`tools/subset_font.py`), вшит в программу — с диска ничего не читается.

use std::sync::OnceLock;

use fontdue::{Font, FontSettings};

use crate::image::Image;
use crate::session::Rect;

static FONT_BYTES: &[u8] = include_bytes!("../../assets/DejaVuSans-Bold-zyrik.ttf");

fn font() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| Font::from_bytes(FONT_BYTES, FontSettings::default()).expect("встроенный шрифт"))
}

/// Размер строки: ширина, высота, расстояние от верха до базовой линии.
pub fn measure(text: &str, size: f32) -> (i32, i32, i32) {
    let f = font();
    let lm = f.horizontal_line_metrics(size).expect("горизонтальные метрики");
    let w: f32 = text.chars().map(|c| f.metrics(c, size).advance_width).sum();
    (w.ceil() as i32, (lm.ascent - lm.descent).ceil() as i32, lm.ascent.ceil() as i32)
}

/// Нарисовать строку, верхний левый угол строки — (x, y). Всё вне `clip` не трогается.
pub fn draw(img: &mut Image, x: i32, y: i32, text: &str, size: f32, color: u32, clip: Rect) {
    let f = font();
    let (_, _, ascent) = measure("", size);
    let baseline = y + ascent;
    let mut pen = x as f32;
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let (cx0, cy0) = (clip.x.max(0), clip.y.max(0));
    let (cx1, cy1) = (clip.right().min(iw), clip.bottom().min(ih));
    let (cr, cg, cb) = ((color >> 16) & 0xFF, (color >> 8) & 0xFF, color & 0xFF);
    let px = img.pixels_mut();
    for ch in text.chars() {
        let (m, cov) = f.rasterize(ch, size);
        let gx = pen.round() as i32 + m.xmin;
        let gy = baseline - m.height as i32 - m.ymin;
        for row in 0..m.height as i32 {
            let y = gy + row;
            if y < cy0 || y >= cy1 {
                continue;
            }
            for col in 0..m.width as i32 {
                let x = gx + col;
                if x < cx0 || x >= cx1 {
                    continue;
                }
                let a = u32::from(cov[(row * m.width as i32 + col) as usize]);
                if a == 0 {
                    continue;
                }
                let d = &mut px[(y * iw + x) as usize];
                let mix = |s: u32, d: u32| (s * a + d * (255 - a)) / 255;
                let (dr, dg, db) = ((*d >> 16) & 0xFF, (*d >> 8) & 0xFF, *d & 0xFF);
                *d = 0xFF00_0000 | (mix(cr, dr) << 16) | (mix(cg, dg) << 8) | mix(cb, db);
            }
        }
        pen += m.advance_width;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cyrillic_and_latin_are_in_the_font() {
        for c in "Зырик Zyrik ёЁ №1 ✓✕ → «»".chars() {
            assert!(c == ' ' || font().lookup_glyph_index(c) != 0, "нет глифа {c:?}");
        }
    }

    #[test]
    fn draws_inside_clip_only() {
        let mut img = Image::from_pixels(60, 30, vec![0xFF00_0000; 1800]);
        let clip = Rect { x: 0, y: 0, w: 30, h: 30 };
        draw(&mut img, 2, 2, "ЖЖЖЖ", 20.0, 0xFFFF_FFFF, clip);
        let lit = |x0: u32, x1: u32| (x0..x1).flat_map(|x| (0..30).map(move |y| (x, y))).filter(|&(x, y)| img.get(x, y) != 0xFF00_0000).count();
        assert!(lit(0, 30) > 20, "текст нарисован");
        assert_eq!(lit(30, 60), 0, "за clip не вылез");
    }
}
