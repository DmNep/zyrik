//! Всё, что видно в режиме выделения, и «запекание» снимка с пометками.
//! Рисуем в свой буфер; платформа только выводит его на экран.

use tiny_skia::{FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

use super::{text, Color, Mark, Shape, Tool, P};
use crate::i18n::{Lang, Msg};
use crate::image::Image;
use crate::session::{Pt, Rect, Session};
use crate::toolbar::{Bar, Button};

/// Синий значка «о_О»
pub const ACCENT: u32 = 0xFF2D_6CDF;
const WHITE: u32 = 0xFFFF_FFFF;
const LABEL_BG: u32 = 0xFF18_1820;
const BAR_BG: u32 = 0xF01C_1C24;
const HOVER_BG: u32 = 0xFF3A_3A48;

const BORDER: i32 = 2;
const HANDLE: i32 = 8;
const FONT_SCALE: i32 = 2;

/// Затемнение вдвое: дёшево, без умножений.
#[inline]
fn dim(px: u32) -> u32 {
    ((px >> 1) & 0x007F_7F7F) | 0xFF00_0000
}

/// Снимок зоны с пометками — именно он идёт в буфер и в файл (T038).
pub fn bake(frame: &Image, sel: Rect, marks: &[Mark], live: Option<&Mark>) -> Image {
    let mut img = frame.crop(sel);
    let all: Vec<&Mark> = marks.iter().chain(live).collect();
    draw_marks(&mut img, sel.x, sel.y, &all);
    img
}

/// Собрать кадр оверлея в `out` (того же размера, что `frame`).
pub fn overlay(frame: &Image, session: &Session, out: &mut Image, lang: Lang) {
    debug_assert_eq!((frame.width(), frame.height()), (out.width(), out.height()));
    {
        let dst = out.pixels_mut();
        for (d, s) in dst.iter_mut().zip(frame.pixels()) {
            *d = dim(*s);
        }
    }
    let Some(sel) = session.selection() else { return };
    let live = session.live();
    let baked = bake(frame, sel, session.marks().items(), live.as_ref());
    // внутри зоны — снимок с пометками, ярко
    let w = out.width() as usize;
    {
        let dst = out.pixels_mut();
        for y in 0..sel.h {
            let row = (sel.y + y) as usize * w + sel.x as usize;
            dst[row..row + sel.w as usize].copy_from_slice(baked.row(y as u32));
        }
    }
    if let Some(e) = session.editing() {
        caret(out, sel, e.at, &e.text, e.size, e.color);
    }
    border(out, sel);
    size_label(out, sel);
    if let Some(bar) = session.toolbar() {
        draw_bar(out, &bar, session);
        if let Some(b) = session.hover() {
            tooltip(out, &bar, sel, b, lang);
        }
    }
    if let Some((p, value, is_text)) = session.preview() {
        preview(out, p, value, is_text, session.color());
    }
}

// ---------- пометки ----------

fn skia_color(argb: u32) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8((argb >> 16) as u8, (argb >> 8) as u8, argb as u8, (argb >> 24) as u8)
}

fn paint(argb: u32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(skia_color(argb));
    p.anti_alias = true;
    p
}

fn stroke(width: f32) -> Stroke {
    Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() }
}

/// Рисовать tiny-skia в прямоугольник `area` картинки: прозрачный холст того же размера,
/// `draw` получает сдвиг из координат кадра (`ox`, `oy` — где у картинки (0, 0)), потом смешивание.
fn with_pixmap(img: &mut Image, area: Rect, draw: impl FnOnce(&mut Pixmap, Transform)) {
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let x0 = area.x.clamp(0, iw);
    let y0 = area.y.clamp(0, ih);
    let x1 = area.right().clamp(0, iw);
    let y1 = area.bottom().clamp(0, ih);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let Some(mut pm) = Pixmap::new((x1 - x0) as u32, (y1 - y0) as u32) else { return };
    draw(&mut pm, Transform::from_translate(-x0 as f32, -y0 as f32));
    let data = pm.data();
    let px = img.pixels_mut();
    let pw = (x1 - x0) as usize;
    for y in 0..(y1 - y0) as usize {
        for x in 0..pw {
            let i = (y * pw + x) * 4;
            let a = u32::from(data[i + 3]);
            if a == 0 {
                continue;
            }
            // tiny-skia: RGBA с умноженной альфой
            let (r, g, b) = (u32::from(data[i]), u32::from(data[i + 1]), u32::from(data[i + 2]));
            let d = &mut px[(y0 as usize + y) * iw as usize + x0 as usize + x];
            let inv = 255 - a;
            let mix = |s: u32, d: u32| (s + (d * inv + 127) / 255).min(255);
            *d = 0xFF00_0000
                | (mix(r, (*d >> 16) & 0xFF) << 16)
                | (mix(g, (*d >> 8) & 0xFF) << 8)
                | mix(b, *d & 0xFF);
        }
    }
}

/// Пометки на картинке, у которой (0, 0) — это точка (ox, oy) кадра.
fn draw_marks(img: &mut Image, ox: i32, oy: i32, marks: &[&Mark]) {
    let full = Rect { x: 0, y: 0, w: img.width() as i32, h: img.height() as i32 };
    with_pixmap(img, full, |pm, _| {
        let t = Transform::from_translate(-ox as f32, -oy as f32);
        for m in marks {
            shape(pm, m, t);
        }
    });
    for m in marks {
        if let Shape::Text { at, text: s, size } = &m.shape {
            let (x, y) = (at.x as i32 - ox, at.y as i32 - oy);
            text::draw(img, x, y, s, *size, m.color.argb(), full);
        }
    }
}

fn shape(pm: &mut Pixmap, m: &Mark, t: Transform) {
    let pnt = paint(m.color.argb());
    let st = stroke(m.width);
    let mut pb = PathBuilder::new();
    match &m.shape {
        Shape::Pen(pts) => {
            if pts.len() == 1 {
                pb.push_circle(pts[0].x, pts[0].y, m.width / 2.0);
                if let Some(path) = pb.finish() {
                    pm.fill_path(&path, &pnt, FillRule::Winding, t, None);
                }
                return;
            }
            pb.move_to(pts[0].x, pts[0].y);
            for p in &pts[1..] {
                pb.line_to(p.x, p.y);
            }
        }
        Shape::Line(a, b) => {
            pb.move_to(a.x, a.y);
            pb.line_to(b.x, b.y);
        }
        Shape::Arrow(a, b) => {
            arrow(pm, *a, *b, m.width, &pnt, t);
            return;
        }
        Shape::Rect(a, b) => {
            let (l, r) = (a.x.min(b.x), a.x.max(b.x));
            let (top, bot) = (a.y.min(b.y), a.y.max(b.y));
            pb.move_to(l, top);
            pb.line_to(r, top);
            pb.line_to(r, bot);
            pb.line_to(l, bot);
            pb.close();
        }
        Shape::Text { .. } => return,
    }
    if let Some(path) = pb.finish() {
        pm.stroke_path(&path, &pnt, &st, t, None);
    }
}

/// Стрелка: древко и залитый наконечник; наконечник растёт с толщиной.
fn arrow(pm: &mut Pixmap, a: P, b: P, width: f32, pnt: &Paint, t: Transform) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let head = (width * 3.0 + 8.0).min(len * 0.8);
    let half = head * 0.55;
    let base = P::new(b.x - ux * head, b.y - uy * head);
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    // древко до середины наконечника: иначе толстое круглое окончание торчит из острия
    pb.line_to(b.x - ux * head * 0.5, b.y - uy * head * 0.5);
    if let Some(path) = pb.finish() {
        pm.stroke_path(&path, pnt, &stroke(width), t, None);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(b.x, b.y);
    pb.line_to(base.x - uy * half, base.y + ux * half);
    pb.line_to(base.x + uy * half, base.y - ux * half);
    pb.close();
    if let Some(path) = pb.finish() {
        pm.fill_path(&path, pnt, FillRule::Winding, t, None);
    }
}

/// Курсор набора подписи: черта после текста.
fn caret(img: &mut Image, sel: Rect, at: P, s: &str, size: f32, color: Color) {
    let (w, h, _) = text::measure(s, size);
    let x = at.x as i32 + w + 1;
    let y = at.y as i32;
    let thick = (size / 14.0).ceil().max(2.0) as i32;
    let clip = |r: Rect| clip_rect(r, sel);
    fill_rect(img, clip(Rect { x, y, w: thick, h }), color.argb());
    if s.is_empty() {
        // пустая подпись: тонкая подсказка, где будет текст
        fill_rect(img, clip(Rect { x: at.x as i32, y: y + h, w: (size * 2.0) as i32, h: 1 }), color.argb());
    }
}

/// Часть `r`, лежащая внутри `bound`.
fn clip_rect(r: Rect, bound: Rect) -> Rect {
    let x0 = r.x.max(bound.x);
    let y0 = r.y.max(bound.y);
    let x1 = r.right().min(bound.right());
    let y1 = r.bottom().min(bound.bottom());
    Rect { x: x0, y: y0, w: (x1 - x0).max(0), h: (y1 - y0).max(0) }
}

// ---------- рамка, размер ----------

fn fill_rect(img: &mut Image, r: Rect, color: u32) {
    fill(img, r.x, r.y, r.w, r.h, color);
}

fn fill(img: &mut Image, x: i32, y: i32, w: i32, h: i32, color: u32) {
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let (x0, y0) = (x.clamp(0, iw), y.clamp(0, ih));
    let (x1, y1) = ((x + w).clamp(0, iw), (y + h).clamp(0, ih));
    let a = color >> 24;
    let px = img.pixels_mut();
    for yy in y0..y1 {
        let row = (yy * iw) as usize;
        let line = &mut px[row + x0 as usize..row + x1 as usize];
        if a == 0xFF {
            line.fill(color);
        } else {
            for d in line {
                let mix = |s: u32, d: u32| (s * a + d * (255 - a)) / 255;
                *d = 0xFF00_0000
                    | (mix((color >> 16) & 0xFF, (*d >> 16) & 0xFF) << 16)
                    | (mix((color >> 8) & 0xFF, (*d >> 8) & 0xFF) << 8)
                    | mix(color & 0xFF, *d & 0xFF);
            }
        }
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

/// «1280 x 720» над левым верхним углом; если там нет места — внутри зоны.
fn size_label(img: &mut Image, s: Rect) {
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

/// Растровый шрифт 5×7 для размера зоны: чёткий на любом фоне и не зависит от масштаба. Бит 4 — левый столбец.
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

// ---------- панель ----------

fn rounded(pb: &mut PathBuilder, r: Rect, rad: f32) {
    let (x, y, w, h) = (r.x as f32, r.y as f32, r.w as f32, r.h as f32);
    let k = 0.552_284_8 * rad;
    pb.move_to(x + rad, y);
    pb.line_to(x + w - rad, y);
    pb.cubic_to(x + w - rad + k, y, x + w, y + rad - k, x + w, y + rad);
    pb.line_to(x + w, y + h - rad);
    pb.cubic_to(x + w, y + h - rad + k, x + w - rad + k, y + h, x + w - rad, y + h);
    pb.line_to(x + rad, y + h);
    pb.cubic_to(x + rad - k, y + h, x, y + h - rad + k, x, y + h - rad);
    pb.line_to(x, y + rad);
    pb.cubic_to(x, y + rad - k, x + rad - k, y, x + rad, y);
    pb.close();
}

fn draw_bar(img: &mut Image, bar: &Bar, s: &Session) {
    let k = bar.scale as f32 / 100.0;
    with_pixmap(img, bar.rect, |pm, t| {
        let mut pb = PathBuilder::new();
        rounded(&mut pb, bar.rect, 8.0 * k);
        if let Some(path) = pb.finish() {
            pm.fill_path(&path, &paint(BAR_BG), FillRule::Winding, t, None);
        }
        for (b, r) in &bar.buttons {
            let active = match b {
                Button::Tool(tool) => s.tool() == Some(*tool),
                _ => false,
            };
            let bg = if active {
                Some(ACCENT)
            } else if s.hover() == Some(*b) {
                Some(HOVER_BG)
            } else {
                None
            };
            if let Some(bg) = bg {
                let mut pb = PathBuilder::new();
                let inset = Rect { x: r.x + 2, y: r.y + 2, w: r.w - 4, h: r.h - 4 };
                rounded(&mut pb, inset, 6.0 * k);
                if let Some(path) = pb.finish() {
                    pm.fill_path(&path, &paint(bg), FillRule::Winding, t, None);
                }
            }
            icon(pm, *b, *r, k, s.color(), t);
        }
    });
}

/// Иконки кнопок — простые векторные значки в квадрате кнопки.
fn icon(pm: &mut Pixmap, b: Button, r: Rect, k: f32, current: Color, t: Transform) {
    let (x, y, w) = (r.x as f32, r.y as f32, r.w as f32);
    // координаты значка — в долях кнопки
    let at = |fx: f32, fy: f32| (x + fx * w, y + fy * w);
    let white = paint(WHITE);
    let line = stroke(2.0 * k);
    let mut pb = PathBuilder::new();
    match b {
        Button::Tool(Tool::Pen) => {
            let (ax, ay) = at(0.25, 0.68);
            pb.move_to(ax, ay);
            let (c1x, c1y) = at(0.38, 0.30);
            let (c2x, c2y) = at(0.52, 0.80);
            let (ex, ey) = at(0.75, 0.32);
            pb.cubic_to(c1x, c1y, c2x, c2y, ex, ey);
        }
        Button::Tool(Tool::Line) => {
            let (ax, ay) = at(0.27, 0.73);
            let (bx, by) = at(0.73, 0.27);
            pb.move_to(ax, ay);
            pb.line_to(bx, by);
        }
        Button::Tool(Tool::Arrow) => {
            let (a, b) = (at(0.27, 0.73), at(0.73, 0.27));
            arrow(pm, P::new(a.0, a.1), P::new(b.0, b.1), 2.0 * k, &white, t);
            return;
        }
        Button::Tool(Tool::Rect) => {
            let (ax, ay) = at(0.26, 0.30);
            let (bx, by) = at(0.74, 0.70);
            pb.move_to(ax, ay);
            pb.line_to(bx, ay);
            pb.line_to(bx, by);
            pb.line_to(ax, by);
            pb.close();
        }
        Button::Tool(Tool::Text) => {
            let (ax, ay) = at(0.30, 0.28);
            let (bx, _) = at(0.70, 0.28);
            let (cx, cy) = at(0.50, 0.74);
            pb.move_to(ax, ay);
            pb.line_to(bx, ay);
            pb.move_to(cx, ay);
            pb.line_to(cx, cy);
        }
        Button::Color(c) => {
            let (cx, cy) = at(0.5, 0.5);
            let mut dot = PathBuilder::new();
            dot.push_circle(cx, cy, w * 0.22);
            if let Some(path) = dot.finish() {
                pm.fill_path(&path, &paint(c.argb()), FillRule::Winding, t, None);
            }
            if c == current {
                pb.push_circle(cx, cy, w * 0.32);
            } else {
                return;
            }
        }
        Button::Undo => {
            // дуга против часовой со стрелкой
            let (sx, sy) = at(0.70, 0.66);
            let (c1x, c1y) = at(0.78, 0.38);
            let (c2x, c2y) = at(0.50, 0.24);
            let (ex, ey) = at(0.30, 0.40);
            pb.move_to(sx, sy);
            pb.cubic_to(c1x, c1y, c2x, c2y, ex, ey);
            let (h1x, h1y) = at(0.28, 0.24);
            let (h2x, h2y) = at(0.45, 0.44);
            pb.move_to(h1x, h1y);
            pb.line_to(ex, ey);
            pb.line_to(h2x, h2y);
        }
        Button::Save => {
            // дискета: корпус со срезанным углом, шторка сверху, этикетка снизу
            let (ax, ay) = at(0.26, 0.26);
            let (bx, by) = at(0.74, 0.74);
            let cut = w * 0.10;
            pb.move_to(ax, ay);
            pb.line_to(bx - cut, ay);
            pb.line_to(bx, ay + cut);
            pb.line_to(bx, by);
            pb.line_to(ax, by);
            pb.close();
            let (sx, _) = at(0.36, 0.0);
            let (ex, shy) = at(0.60, 0.40);
            pb.move_to(sx, ay);
            pb.line_to(sx, shy);
            pb.line_to(ex, shy);
            pb.line_to(ex, ay);
            let (lx, ly) = at(0.36, 0.58);
            let (rx, _) = at(0.64, 0.0);
            pb.move_to(lx, ly);
            pb.line_to(rx, ly);
            pb.line_to(rx, by);
            pb.move_to(lx, ly);
            pb.line_to(lx, by);
        }
        Button::Copy => {
            let (ax, ay) = at(0.26, 0.52);
            let (bx, by) = at(0.43, 0.69);
            let (cx, cy) = at(0.75, 0.31);
            pb.move_to(ax, ay);
            pb.line_to(bx, by);
            pb.line_to(cx, cy);
        }
        Button::Close => {
            let (ax, ay) = at(0.32, 0.32);
            let (bx, by) = at(0.68, 0.68);
            pb.move_to(ax, ay);
            pb.line_to(bx, by);
            pb.move_to(bx, ay);
            pb.line_to(ax, by);
        }
    }
    if let Some(path) = pb.finish() {
        let st = if matches!(b, Button::Copy) { stroke(2.6 * k) } else { line };
        pm.stroke_path(&path, &white, &st, t, None);
    }
}

fn tip_text(b: Button, lang: Lang) -> &'static str {
    lang.tr(match b {
        Button::Tool(Tool::Pen) => Msg::TipPen,
        Button::Tool(Tool::Line) => Msg::TipLine,
        Button::Tool(Tool::Arrow) => Msg::TipArrow,
        Button::Tool(Tool::Rect) => Msg::TipRect,
        Button::Tool(Tool::Text) => Msg::TipText,
        Button::Color(Color::Red) => Msg::TipRed,
        Button::Color(Color::Blue) => Msg::TipBlue,
        Button::Undo => Msg::TipUndo,
        Button::Save => Msg::TipSave,
        Button::Copy => Msg::TipCopy,
        Button::Close => Msg::TipClose,
    })
}

/// Подсказка «название (клавиша)» со стороны панели, дальней от зоны: на выделенное не ложится.
fn tooltip(img: &mut Image, bar: &Bar, sel: Rect, b: Button, lang: Lang) {
    let Some(r) = bar.button_rect(b) else { return };
    let k = bar.scale as f32 / 100.0;
    let size = 14.0 * k;
    let s = tip_text(b, lang);
    let (tw, th, _) = text::measure(s, size);
    let pad = (6.0 * k) as i32;
    let (w, h) = (tw + 2 * pad, th + pad);
    let iw = img.width() as i32;
    let x = (r.x + r.w / 2 - w / 2).clamp(0, (iw - w).max(0));
    let (ih, gap) = (img.height() as i32, 4);
    let below = bar.rect.bottom() + gap;
    let above = bar.rect.y - h - gap;
    let prefer_below = bar.rect.y >= sel.bottom();
    let y = match (prefer_below, below + h <= ih, above >= 0) {
        (true, true, _) | (false, true, false) => below,
        _ => above.max(0),
    };
    fill(img, x, y, w, h, LABEL_BG);
    let full = Rect { x: 0, y: 0, w: iw, h: img.height() as i32 };
    text::draw(img, x + pad, y + pad / 2, s, size, WHITE, full);
}

/// Превью толщины у курсора: кружок того диаметра, что будет линия, и число;
/// для подписи — «Аа» нужного размера.
fn preview(img: &mut Image, p: Pt, value: f32, is_text: bool, color: Color) {
    let full = Rect { x: 0, y: 0, w: img.width() as i32, h: img.height() as i32 };
    let (x, y) = (p.x + 18, p.y + 18);
    if is_text {
        let (tw, th, _) = text::measure("Аа", value);
        fill(img, x - 4, y - 2, tw + 8, th + 4, 0xC018_1820);
        text::draw(img, x, y, "Аа", value, color.argb(), full);
        return;
    }
    let d = value.max(2.0);
    let area = Rect { x: x - 2, y: y - 2, w: d as i32 + 6, h: d as i32 + 6 };
    with_pixmap(img, area, |pm, t| {
        let mut pb = PathBuilder::new();
        pb.push_circle(x as f32 + d / 2.0 + 1.0, y as f32 + d / 2.0 + 1.0, d / 2.0);
        if let Some(path) = pb.finish() {
            pm.fill_path(&path, &paint(color.argb()), FillRule::Winding, t, None);
            pm.stroke_path(&path, &paint(WHITE), &stroke(1.0), t, None);
        }
    });
    let label = format!("{}", value as i32);
    let (_, th, _) = text::measure(&label, 14.0);
    let lx = x + d as i32 + 8;
    let ly = y + d as i32 / 2 - th / 2;
    let (lw, _, _) = text::measure(&label, 14.0);
    fill(img, lx - 3, ly - 1, lw + 6, th + 2, 0xC018_1820);
    text::draw(img, lx, ly, &label, 14.0, WHITE, full);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Button as MouseButton, Input};

    fn frame(w: u32, h: u32) -> Image {
        let px = (0..w * h).map(|i| 0xFF00_0000 | (i.wrapping_mul(2_654_435_761) % 0xFF_FFFF)).collect();
        Image::from_pixels(w, h, px)
    }

    fn en() -> Lang {
        Lang::english()
    }

    #[test]
    fn without_selection_everything_is_dimmed() {
        let f = frame(64, 48);
        let s = Session::new(64, 48);
        let mut out = Image::new(64, 48);
        overlay(&f, &s, &mut out, en());
        assert!(out.pixels().iter().zip(f.pixels()).all(|(o, p)| *o == dim(*p)));
    }

    #[test]
    fn selection_keeps_original_pixels_and_border_stays_outside() {
        let f = frame(400, 300);
        let mut s = Session::new(400, 300);
        s.input(Input::Down(MouseButton::Left, Pt { x: 60, y: 40 }));
        s.input(Input::Up(MouseButton::Left, Pt { x: 200, y: 140 }));
        let sel = s.selection().unwrap();
        let mut out = Image::new(400, 300);
        overlay(&f, &s, &mut out, en());
        // внутри, кроме ручек, которые заходят внутрь на полручки, — исходные пиксели
        let inner = Rect { x: sel.x + HANDLE, y: sel.y + HANDLE, w: sel.w - 2 * HANDLE, h: sel.h - 2 * HANDLE };
        for y in inner.y..inner.bottom() {
            for x in inner.x..inner.right() {
                assert_eq!(out.get(x as u32, y as u32), f.get(x as u32, y as u32), "{x},{y}");
            }
        }
        assert_eq!(out.get((sel.x + sel.w / 4) as u32, (sel.y - 1) as u32), ACCENT);
        assert_eq!(out.get(5, 295), dim(f.get(5, 295)));
    }

    #[test]
    fn bake_draws_marks_in_zone_coordinates() {
        let f = Image::from_pixels(100, 100, vec![0xFF00_0000; 10_000]);
        let sel = Rect { x: 20, y: 30, w: 50, h: 40 };
        // горизонтальная красная линия по y = 50 кадра → y = 20 снимка
        let m = Mark { shape: Shape::Line(P::new(25.0, 50.0), P::new(65.0, 50.0)), color: Color::Red, width: 6.0 };
        let img = bake(&f, sel, std::slice::from_ref(&m), None);
        assert_eq!((img.width(), img.height()), (50, 40));
        let on = img.get(25, 20);
        assert!((on >> 16) & 0xFF > 200 && on & 0xFF < 80, "красный на линии: {on:#x}");
        assert_eq!(img.get(25, 5), 0xFF00_0000, "вдали от линии — исходный пиксель");
    }

    #[test]
    fn baked_text_is_in_the_shot() {
        let f = Image::from_pixels(200, 80, vec![0xFF00_0000; 16_000]);
        let sel = Rect { x: 0, y: 0, w: 200, h: 80 };
        let m = Mark { shape: Shape::Text { at: P::new(10.0, 10.0), text: "Тут".into(), size: 30.0 }, color: Color::Blue, width: 4.0 };
        let img = bake(&f, sel, &[m], None);
        assert!(img.pixels().iter().any(|p| *p & 0xFF > 200), "синий текст есть");
    }
}
