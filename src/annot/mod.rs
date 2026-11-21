//! Пометки на снимке (история 2): модель, привязка Shift, отмена и возврат.
//! Координаты — физические пиксели кадра, как у выделения.

pub mod render;
pub mod text;

use zeroize::Zeroize;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct P {
    pub x: f32,
    pub y: f32,
}

impl P {
    pub fn new(x: f32, y: f32) -> P {
        P { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Line,
    Arrow,
    Rect,
    Text,
}

impl Tool {
    /// Клавиша-дубль (принцип III)
    pub fn key(self) -> char {
        match self {
            Tool::Pen => 'P',
            Tool::Line => 'L',
            Tool::Arrow => 'A',
            Tool::Rect => 'R',
            Tool::Text => 'T',
        }
    }

    pub fn from_key(c: char) -> Option<Tool> {
        [Tool::Pen, Tool::Line, Tool::Arrow, Tool::Rect, Tool::Text]
            .into_iter()
            .find(|t| t.key() == c.to_ascii_uppercase())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Red,
    Blue,
}

impl Color {
    /// 0xAARRGGBB
    pub fn argb(self) -> u32 {
        match self {
            Color::Red => 0xFFE5_2B2B,
            Color::Blue => 0xFF1F_6FEB,
        }
    }

    pub fn other(self) -> Color {
        match self {
            Color::Red => Color::Blue,
            Color::Blue => Color::Red,
        }
    }
}

pub const MIN_WIDTH: f32 = 1.0;
pub const MAX_WIDTH: f32 = 40.0;
pub const DEFAULT_WIDTH: f32 = 4.0;
pub const MIN_TEXT: f32 = 10.0;
pub const MAX_TEXT: f32 = 120.0;
pub const DEFAULT_TEXT: f32 = 24.0;
/// Шаг размера подписи на один щелчок колёсика
pub const TEXT_STEP: f32 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Pen(Vec<P>),
    Line(P, P),
    Arrow(P, P),
    /// Два противоположных угла
    Rect(P, P),
    Text { at: P, text: String, size: f32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    pub shape: Shape,
    pub color: Color,
    /// Толщина линии в пикселях
    pub width: f32,
}

impl Mark {
    /// Пометка, которую стоит сохранять: не точка-щелчок и не пустая подпись.
    pub fn is_visible(&self) -> bool {
        let far = |a: P, b: P| (a.x - b.x).abs() >= 2.0 || (a.y - b.y).abs() >= 2.0;
        match &self.shape {
            Shape::Pen(pts) => !pts.is_empty(),
            Shape::Line(a, b) | Shape::Arrow(a, b) | Shape::Rect(a, b) => far(*a, *b),
            Shape::Text { text, .. } => !text.trim().is_empty(),
        }
    }
}

/// Пометки и стек отмены. Текст подписей затирается при удалении.
#[derive(Debug, Default)]
pub struct Marks {
    items: Vec<Mark>,
    undone: Vec<Mark>,
}

impl Marks {
    pub fn new() -> Marks {
        Marks::default()
    }

    /// Новая пометка; то, что было отменено, вернуть уже нельзя.
    pub fn push(&mut self, m: Mark) {
        if m.is_visible() {
            self.items.push(m);
            wipe_all(&mut self.undone);
        }
    }

    pub fn undo(&mut self) -> bool {
        match self.items.pop() {
            Some(m) => {
                self.undone.push(m);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.undone.pop() {
            Some(m) => {
                self.items.push(m);
                true
            }
            None => false,
        }
    }

    pub fn items(&self) -> &[Mark] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

fn wipe_all(v: &mut Vec<Mark>) {
    for m in v.iter_mut() {
        if let Shape::Text { text, .. } = &mut m.shape {
            text.zeroize();
        }
    }
    v.clear();
}

impl Drop for Marks {
    fn drop(&mut self) {
        wipe_all(&mut self.items);
        wipe_all(&mut self.undone);
    }
}

/// Shift у линии и стрелки: конец привязывается к ближайшему из 0°, 45°, 90°… (длина та же).
pub fn snap45(a: P, b: P) -> P {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        return b;
    }
    let step = std::f32::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    let (s, c) = angle.sin_cos();
    // убираем дрожание float у осей: cos(90°) = 6e-8, а не 0
    let clean = |v: f32| if v.abs() < 1e-4 { 0.0 } else { v };
    P::new(a.x + clean(c) * len, a.y + clean(s) * len)
}

/// Shift у прямоугольника: квадрат по большей стороне, в сторону курсора.
pub fn square(a: P, b: P) -> P {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let side = dx.abs().max(dy.abs());
    P::new(a.x + side.copysign(dx), a.y + side.copysign(dy))
}

pub fn clamp_width(w: f32) -> f32 {
    w.clamp(MIN_WIDTH, MAX_WIDTH)
}

pub fn clamp_text(s: f32) -> f32 {
    s.clamp(MIN_TEXT, MAX_TEXT)
}
