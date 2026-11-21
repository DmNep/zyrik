//! Режим выделения (история 1): зона, ручки рамки, перемещение, клавиши.
//! Координаты — физические пиксели снимка, (0, 0) — левый верхний угол виртуального экрана.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pt {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn right(self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(self) -> i32 {
        self.y + self.h
    }

    pub fn contains(self, p: Pt) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// Прямоугольник по двум противоположным углам, в любом порядке.
    fn from_edges(l: i32, t: i32, r: i32, b: i32) -> Rect {
        let (l, r) = (l.min(r), l.max(r));
        let (t, b) = (t.min(b), t.max(b));
        Rect { x: l, y: t, w: r - l, h: b - t }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Esc,
    Enter,
    /// Буква C: с Ctrl — копировать
    C,
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Down(Button, Pt),
    Move(Pt),
    Up(Button, Pt),
    DoubleClick(Pt),
    Key { key: Key, shift: bool, ctrl: bool },
    /// Окно просит перерисовку (ОС стёрла содержимое)
    Repaint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Redraw,
    /// Готово: снять эту зону
    Confirm(Rect),
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    Cross,
    Move,
    /// ↖↘
    ResizeNwse,
    /// ↗↙
    ResizeNesw,
    /// ↕
    ResizeNs,
    /// ↔
    ResizeWe,
}

/// Какие края тянет ручка.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Edges {
    left: bool,
    top: bool,
    right: bool,
    bottom: bool,
}

#[derive(Debug, Clone, Copy)]
enum Drag {
    None,
    /// Протягиваем новую зону. Пока сдвиг меньше порога — старая зона остаётся.
    Creating { anchor: Pt, started: bool },
    Moving { grab: Pt, start: Rect },
    Resizing { edges: Edges, grab: Pt, start: Rect },
}

/// Насколько надо сдвинуть мышь, чтобы щелчок стал протягиванием.
const DRAG_THRESHOLD: i32 = 3;
/// Насколько близко к краю рамки начинается ручка.
const HANDLE_REACH: i32 = 8;
const BIG_STEP: i32 = 10;

pub struct Session {
    width: i32,
    height: i32,
    sel: Option<Rect>,
    drag: Drag,
}

impl Session {
    pub fn new(width: u32, height: u32) -> Session {
        Session { width: width as i32, height: height as i32, sel: None, drag: Drag::None }
    }

    pub fn selection(&self) -> Option<Rect> {
        self.sel
    }

    /// Курсор во время текущего действия (или над точкой, если ничего не тянем).
    pub fn cursor(&self) -> Cursor {
        match self.drag {
            Drag::Creating { .. } | Drag::None => Cursor::Cross,
            Drag::Moving { .. } => Cursor::Move,
            Drag::Resizing { edges, .. } => edges_cursor(edges),
        }
    }

    /// Какой курсор показать над точкой `p`.
    pub fn cursor_at(&self, p: Pt) -> Cursor {
        if !matches!(self.drag, Drag::None) {
            return self.cursor();
        }
        match self.sel {
            Some(sel) => match hit(sel, p) {
                Some(edges) => edges_cursor(edges),
                None if sel.contains(p) => Cursor::Move,
                None => Cursor::Cross,
            },
            None => Cursor::Cross,
        }
    }

    fn clamp_pt(&self, p: Pt) -> Pt {
        Pt { x: p.x.clamp(0, self.width), y: p.y.clamp(0, self.height) }
    }

    /// Сдвинуть зону, не выпуская за экран.
    fn place(&self, r: Rect) -> Rect {
        let w = r.w.min(self.width);
        let h = r.h.min(self.height);
        Rect { x: r.x.clamp(0, self.width - w), y: r.y.clamp(0, self.height - h), w, h }
    }

    pub fn input(&mut self, input: Input) -> Outcome {
        match input {
            Input::Repaint => Outcome::Redraw,
            Input::Down(Button::Right, _) => {
                self.drag = Drag::None;
                if self.sel.take().is_some() {
                    Outcome::Redraw
                } else {
                    Outcome::Cancel
                }
            }
            Input::Down(Button::Left, p) => {
                let p = self.clamp_pt(p);
                self.drag = match self.sel {
                    Some(sel) => match hit(sel, p) {
                        Some(edges) => Drag::Resizing { edges, grab: p, start: sel },
                        None if sel.contains(p) => Drag::Moving { grab: p, start: sel },
                        None => Drag::Creating { anchor: p, started: false },
                    },
                    None => Drag::Creating { anchor: p, started: false },
                };
                Outcome::Nothing
            }
            Input::Move(p) => self.drag_to(p),
            Input::Up(Button::Left, p) => {
                let out = self.drag_to(p);
                self.drag = Drag::None;
                out
            }
            Input::Up(Button::Right, _) => Outcome::Nothing,
            Input::DoubleClick(p) => match self.sel {
                Some(sel) if sel.contains(p) => Outcome::Confirm(sel),
                _ => Outcome::Nothing,
            },
            Input::Key { key, shift, ctrl } => self.key(key, shift, ctrl),
        }
    }

    fn drag_to(&mut self, p: Pt) -> Outcome {
        let p = self.clamp_pt(p);
        match self.drag {
            Drag::None => Outcome::Nothing,
            Drag::Creating { anchor, started } => {
                let far = (p.x - anchor.x).abs().max((p.y - anchor.y).abs()) >= DRAG_THRESHOLD;
                if !started && !far {
                    return Outcome::Nothing;
                }
                self.drag = Drag::Creating { anchor, started: true };
                self.sel = Some(Rect::from_edges(anchor.x, anchor.y, p.x, p.y));
                Outcome::Redraw
            }
            Drag::Moving { grab, start } => {
                let moved = Rect { x: start.x + p.x - grab.x, y: start.y + p.y - grab.y, ..start };
                self.sel = Some(self.place(moved));
                Outcome::Redraw
            }
            Drag::Resizing { edges, grab, start } => {
                let (dx, dy) = (p.x - grab.x, p.y - grab.y);
                let mut l = start.x;
                let mut t = start.y;
                let mut r = start.right();
                let mut b = start.bottom();
                if edges.left {
                    l += dx;
                }
                if edges.right {
                    r += dx;
                }
                if edges.top {
                    t += dy;
                }
                if edges.bottom {
                    b += dy;
                }
                let mut sel = Rect::from_edges(
                    l.clamp(0, self.width),
                    t.clamp(0, self.height),
                    r.clamp(0, self.width),
                    b.clamp(0, self.height),
                );
                sel.w = sel.w.max(1);
                sel.h = sel.h.max(1);
                self.sel = Some(self.place(sel));
                Outcome::Redraw
            }
        }
    }

    fn key(&mut self, key: Key, shift: bool, ctrl: bool) -> Outcome {
        match key {
            Key::Esc => return Outcome::Cancel,
            Key::Enter => return self.sel.map_or(Outcome::Nothing, Outcome::Confirm),
            Key::C if ctrl => return self.sel.map_or(Outcome::Nothing, Outcome::Confirm),
            Key::C => return Outcome::Nothing,
            _ => {}
        }
        let Some(sel) = self.sel else { return Outcome::Nothing };
        let step = if shift { BIG_STEP } else { 1 };
        let (dx, dy) = match key {
            Key::Left => (-step, 0),
            Key::Right => (step, 0),
            Key::Up => (0, -step),
            Key::Down => (0, step),
            _ => (0, 0),
        };
        self.sel = Some(if ctrl {
            // Ctrl+стрелки тянут правый и нижний края
            let w = (sel.w + dx).clamp(1, self.width - sel.x);
            let h = (sel.h + dy).clamp(1, self.height - sel.y);
            Rect { w, h, ..sel }
        } else {
            self.place(Rect { x: sel.x + dx, y: sel.y + dy, ..sel })
        });
        Outcome::Redraw
    }
}

/// Попала ли точка на ручку рамки: углы важнее краёв.
fn hit(sel: Rect, p: Pt) -> Option<Edges> {
    let near = |a: i32, b: i32| (a - b).abs() <= HANDLE_REACH;
    let in_x = p.x >= sel.x - HANDLE_REACH && p.x <= sel.right() + HANDLE_REACH;
    let in_y = p.y >= sel.y - HANDLE_REACH && p.y <= sel.bottom() + HANDLE_REACH;
    if !in_x || !in_y {
        return None;
    }
    let e = Edges {
        left: near(p.x, sel.x),
        right: near(p.x, sel.right()) && !near(p.x, sel.x),
        top: near(p.y, sel.y),
        bottom: near(p.y, sel.bottom()) && !near(p.y, sel.y),
    };
    (e.left || e.right || e.top || e.bottom).then_some(e)
}

fn edges_cursor(e: Edges) -> Cursor {
    match (e.left || e.right, e.top || e.bottom) {
        (true, true) if (e.left && e.top) || (e.right && e.bottom) => Cursor::ResizeNwse,
        (true, true) => Cursor::ResizeNesw,
        (true, false) => Cursor::ResizeWe,
        _ => Cursor::ResizeNs,
    }
}
