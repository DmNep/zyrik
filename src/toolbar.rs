//! Панель инструментов: где стоит и какая кнопка под курсором. Рисует её `annot::render`.
//!
//! Слева направо: 5 инструментов · красный, синий · отменить, сохранить как… · в буфер (✓), закрыть (✕).
//! Закраска (история 4, P2) появится здесь в фазе 5.
//! Стоит вплотную к рамке снаружи, снизу справа; не влезает — сверху; не влезает и там — внутри рамки.

use crate::annot::{Color, Tool};
use crate::platform::Monitor;
use crate::session::{Pt, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Tool(Tool),
    Color(Color),
    Undo,
    /// Дискета: «Сохранить как…»
    Save,
    /// ✓ — в буфер
    Copy,
    /// ✕ — закрыть
    Close,
}

/// Кнопки по группам; между группами — промежуток.
pub const GROUPS: [&[Button]; 4] = [
    &[
        Button::Tool(Tool::Pen),
        Button::Tool(Tool::Line),
        Button::Tool(Tool::Arrow),
        Button::Tool(Tool::Rect),
        Button::Tool(Tool::Text),
    ],
    &[Button::Color(Color::Red), Button::Color(Color::Blue)],
    &[Button::Undo, Button::Save],
    &[Button::Copy, Button::Close],
];

const BUTTON: i32 = 32;
const PAD: i32 = 4;
const GROUP_GAP: i32 = 10;
/// Отступ от рамки: рамка выделения рисуется снаружи зоны толщиной 2 px
pub const GAP: i32 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    pub rect: Rect,
    pub buttons: Vec<(Button, Rect)>,
    /// Масштаб монитора, на котором панель, %
    pub scale: u32,
}

impl Bar {
    pub fn hit(&self, p: Pt) -> Option<Button> {
        self.buttons.iter().find(|(_, r)| r.contains(p)).map(|(b, _)| *b)
    }

    pub fn contains(&self, p: Pt) -> bool {
        self.rect.contains(p)
    }

    pub fn button_rect(&self, b: Button) -> Option<Rect> {
        self.buttons.iter().find(|(x, _)| *x == b).map(|(_, r)| *r)
    }
}

fn s(v: i32, scale: u32) -> i32 {
    (v * scale as i32 + 50) / 100
}

/// Монитор, на котором правый нижний угол зоны; если такого нет — весь экран.
pub fn monitor_of(sel: Rect, screen: Rect, monitors: &[Monitor]) -> Monitor {
    let corner = Pt { x: sel.right() - 1, y: sel.bottom() - 1 };
    monitors
        .iter()
        .find(|m| m.rect.contains(corner))
        .copied()
        .unwrap_or(Monitor { rect: screen, scale: 100 })
}

pub fn layout(sel: Rect, screen: Rect, monitors: &[Monitor]) -> Bar {
    let mon = monitor_of(sel, screen, monitors);
    let scale = mon.scale.max(100);
    let (btn, pad, gg, gap) = (s(BUTTON, scale), s(PAD, scale), s(GROUP_GAP, scale), s(GAP, scale));
    let count: i32 = GROUPS.iter().map(|g| g.len() as i32).sum();
    let w = pad * 2 + btn * count + gg * (GROUPS.len() as i32 - 1);
    let h = pad * 2 + btn;
    let m = mon.rect;

    let x = (sel.right() - w).clamp(m.x, (m.right() - w).max(m.x));
    let below = sel.bottom() + gap;
    let above = sel.y - gap - h;
    let y = if below + h <= m.bottom() {
        below
    } else if above >= m.y {
        above
    } else {
        // зона на весь экран: внутри рамки, у нижнего края
        (sel.bottom() - gap - h).max(m.y)
    };

    let mut buttons = Vec::with_capacity(count as usize);
    let mut bx = x + pad;
    for (gi, group) in GROUPS.iter().enumerate() {
        if gi > 0 {
            bx += gg;
        }
        for b in group.iter() {
            buttons.push((*b, Rect { x: bx, y: y + pad, w: btn, h: btn }));
            bx += btn;
        }
    }
    Bar { rect: Rect { x, y, w, h }, buttons, scale }
}
