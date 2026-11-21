//! Картинка в памяти. Пиксель — `u32` 0xAARRGGBB: в памяти это байты B, G, R, A,
//! ровно как у 32-битного DIB Windows, поэтому захват и буфер обмена обходятся без перестановки.
//!
//! FR-009: при удалении (и по `wipe`) пиксели затираются нулями.

use zeroize::Zeroize;

use crate::session::Rect;

pub struct Image {
    width: u32,
    height: u32,
    px: Vec<u32>,
}

impl Image {
    pub fn new(width: u32, height: u32) -> Image {
        Image { width, height, px: vec![0; width as usize * height as usize] }
    }

    /// Забирает готовый вектор пикселей (длина должна быть width × height).
    pub fn from_pixels(width: u32, height: u32, px: Vec<u32>) -> Image {
        assert_eq!(px.len(), width as usize * height as usize, "размер картинки");
        Image { width, height, px }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u32] {
        &self.px
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.px
    }

    pub fn get(&self, x: u32, y: u32) -> u32 {
        self.px[(y * self.width + x) as usize]
    }

    /// Строка пикселей `y`.
    pub fn row(&self, y: u32) -> &[u32] {
        let w = self.width as usize;
        &self.px[y as usize * w..(y as usize + 1) * w]
    }

    /// Копия прямоугольника (в координатах картинки), пиксель в пиксель (FR-012).
    /// Часть, вылезающая за картинку, отрезается.
    pub fn crop(&self, r: Rect) -> Image {
        let x0 = r.x.clamp(0, self.width as i32) as u32;
        let y0 = r.y.clamp(0, self.height as i32) as u32;
        let x1 = (r.x + r.w).clamp(0, self.width as i32) as u32;
        let y1 = (r.y + r.h).clamp(0, self.height as i32) as u32;
        let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        let mut px = Vec::with_capacity(w as usize * h as usize);
        for y in y0..y1 {
            px.extend_from_slice(&self.row(y)[x0 as usize..x1 as usize]);
        }
        Image { width: w, height: h, px }
    }

    /// Затереть нулями, не освобождая память.
    pub fn wipe(&mut self) {
        self.px.zeroize();
        // zeroize для Vec ещё и обнуляет длину — возвращаем размер, но уже с нулями
        self.px.resize(self.width as usize * self.height as usize, 0);
    }
}

impl Drop for Image {
    fn drop(&mut self) {
        self.px.zeroize();
    }
}

impl std::fmt::Debug for Image {
    // пиксели не печатаем никогда
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}×{})", self.width, self.height)
    }
}
