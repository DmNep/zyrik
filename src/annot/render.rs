//! Картинка режима выделения: «замороженный» кадр, затемнение вне зоны, рамка с ручками,
//! размер зоны в пикселях. Рисуем прямо в буфер; платформа только выводит его на экран.

use crate::image::Image;
use crate::session::{Rect, Session};

/// Синий значка «о_О»
pub const ACCENT: u32 = 0xFF2D_6CDF;
const WHITE: u32 = 0xFFFF_FFFF;
const LABEL_BG: u32 = 0xFF18_1820;

const BORDER: i32 = 2;
const HANDLE: i32 = 8;
const FONT_SCALE: i32 = 2;

/// Затемнение вдвое: дёшево, без умножений.
#[inline]
fn dim(px: u32) -> u32 {
    ((px >> 1) & 0x007F_7F7F) | 0xFF00_0000
}

/// Собрать кадр оверлея в `out` (того же размера, что `frame`).
pub fn overlay(frame: &Image, session: &Session, out: &mut Image) {
    debug_assert_eq!((frame.width(), frame.height()), (out.width(), out.height()));
    let w = frame.width() as usize;
    let src = frame.pixels();
    let dst = out.pixels_mut();
    for (d, s) in dst.iter_mut().zip(src) {
        *d = dim(*s);
    }
    let Some(sel) = session.selection() else { return };
    // внутри зоны — исходные пиксели
    for y in sel.y..sel.bottom() {
        let row = y as usize * w;
        let (a, b) = (row + sel.x as usize, row + sel.right() as usize);
        dst[a..b].copy_from_slice(&src[a..b]);
    }
    border(out, sel);
    label(out, sel);
}

fn fill(img: &mut Image, x: i32, y: i32, w: i32, h: i32, color: u32) {
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let (x0, y0) = (x.clamp(0, iw), y.clamp(0, ih));
    let (x1, y1) = ((x + w).clamp(0, iw), (y + h).clamp(0, ih));
    let px = img.pixels_mut();
    for yy in y0..y1 {
        let row = (yy * iw) as usize;
        px[row + x0 as usize..row + x1 as usize].fill(color);
    }
}

/// Рамка снаружи зоны (сама зона не закрывается) и восемь ручек.
fn border(img: &mut Image, s: Rect) {
    let b = BORDER;
    fill(img, s.x - b, s.y - b, s.w + 2 * b, b, ACCENT);
    fill(img, s.x - b, s.bottom(), s.w + 2 * b, b, ACCENT);
    fill(img, s.x - b, s.y, b, s.h, ACCENT);
    fill(img, s.right(), s.y, b, s.h, ACCENT);
    let (cx, cy) = (s.x + s.w / 2, s.y + s.h / 2);
    for (x, y) in [
        (s.x, s.y), (cx, s.y), (s.right(), s.y),
        (s.x, cy), (s.right(), cy),
        (s.x, s.bottom()), (cx, s.bottom()), (s.right(), s.bottom()),
    ] {
        let h = HANDLE;
        fill(img, x - h / 2 - 1, y - h / 2 - 1, h + 2, h + 2, ACCENT);
        fill(img, x - h / 2, y - h / 2, h, h, WHITE);
    }
}

/// «1280 × 720» над левым верхним углом; если там нет места — внутри зоны.
fn label(img: &mut Image, s: Rect) {
    let text = format!("{} x {}", s.w, s.h);
    let (gw, gh) = (GLYPH_W * FONT_SCALE, GLYPH_H * FONT_SCALE);
    let pad = 4;
    let tw = text.len() as i32 * (gw + FONT_SCALE) - FONT_SCALE;
    let (bw, bh) = (tw + 2 * pad, gh + 2 * pad);
    let gap = BORDER + 4;
    let mut y = s.y - gap - bh;
    if y < 0 {
        y = s.y + gap;
    }
    let x = s.x.clamp(0, (img.width() as i32 - bw).max(0));
    fill(img, x, y, bw, bh, LABEL_BG);
    let mut cx = x + pad;
    for ch in text.chars() {
        draw_glyph(img, ch, cx, y + pad);
        cx += gw + FONT_SCALE;
    }
}

const GLYPH_W: i32 = 5;
const GLYPH_H: i32 = 7;

/// Растровый шрифт 5×7: только то, что нужно для размера зоны. Бит 4 — левый столбец.
fn glyph(ch: char) -> [u8; 7] {
    match ch {
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        'x' => [0b00000, 0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001],
        _ => [0; 7],
    }
}

fn draw_glyph(img: &mut Image, ch: char, x: i32, y: i32) {
    for (row, bits) in glyph(ch).iter().enumerate() {
        for col in 0..GLYPH_W {
            if bits & (1 << (GLYPH_W - 1 - col)) != 0 {
                fill(img, x + col * FONT_SCALE, y + row as i32 * FONT_SCALE, FONT_SCALE, FONT_SCALE, WHITE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Button, Input, Pt};

    fn frame(w: u32, h: u32) -> Image {
        let px = (0..w * h).map(|i| 0xFF00_0000 | (i.wrapping_mul(2_654_435_761) % 0xFF_FFFF)).collect();
        Image::from_pixels(w, h, px)
    }

    #[test]
    fn without_selection_everything_is_dimmed() {
        let f = frame(64, 48);
        let s = Session::new(64, 48);
        let mut out = Image::new(64, 48);
        overlay(&f, &s, &mut out);
        assert!(out.pixels().iter().zip(f.pixels()).all(|(o, p)| *o == dim(*p)));
    }

    #[test]
    fn selection_keeps_original_pixels_and_border_stays_outside() {
        let f = frame(200, 150);
        let mut s = Session::new(200, 150);
        s.input(Input::Down(Button::Left, Pt { x: 60, y: 70 }));
        s.input(Input::Up(Button::Left, Pt { x: 140, y: 120 }));
        let sel = s.selection().unwrap();
        let mut out = Image::new(200, 150);
        overlay(&f, &s, &mut out);
        // внутри, кроме ручек, которые заходят внутрь на полручки, — исходные пиксели
        let inner = Rect { x: sel.x + HANDLE, y: sel.y + HANDLE, w: sel.w - 2 * HANDLE, h: sel.h - 2 * HANDLE };
        for y in inner.y..inner.bottom() {
            for x in inner.x..inner.right() {
                assert_eq!(out.get(x as u32, y as u32), f.get(x as u32, y as u32), "{x},{y}");
            }
        }
        // рамка снаружи зоны
        assert_eq!(out.get((sel.x + sel.w / 4) as u32, (sel.y - 1) as u32), ACCENT);
        // далеко от зоны — затемнение
        assert_eq!(out.get(5, 145), dim(f.get(5, 145)));
    }
}
