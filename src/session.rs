//! Режим выделения (истории 1–3): зона и рамка, панель, пометки, подписи, клавиши.
//! Координаты — физические пиксели снимка, (0, 0) — левый верхний угол виртуального экрана.

use crate::annot::{self, Color, Mark, Marks, Shape, Tool, P};
use crate::platform::Monitor;
use crate::redact::Finding;
use crate::toolbar::{self, Bar, Button as BarButton};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pt {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
    Backspace,
    Left,
    Right,
    Up,
    Down,
    /// Буквенная клавиша A–Z (по виртуальному коду, не зависит от раскладки)
    Letter(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    Down(Button, Pt),
    Move(Pt),
    Up(Button, Pt),
    DoubleClick(Pt),
    Key { key: Key, shift: bool, ctrl: bool },
    /// Набранный символ (для подписи), уже с учётом раскладки
    Char(char),
    /// Колёсико: щелчки (+ от себя), где курсор
    Wheel(i32, Pt),
    /// Нажат или отпущен Shift — меняется привязка линий
    Shift(bool),
    /// Окно просит перерисовку (ОС стёрла содержимое)
    Repaint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Redraw,
    /// Готово: в буфер
    Confirm(Rect),
    /// Ctrl+S: в файл в последнюю папку, без диалога
    Save(Rect),
    /// Дискета: диалог «Сохранить как…»
    SaveAs(Rect),
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cursor {
    Cross,
    Arrow,
    Text,
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

#[derive(Debug, Clone)]
enum Drag {
    None,
    /// Протягиваем новую зону. Пока сдвиг меньше порога — старая зона остаётся.
    Creating { anchor: Pt, started: bool },
    Moving { grab: Pt, start: Rect },
    Resizing { edges: Edges, grab: Pt, start: Rect },
    /// Рисуем пометку: начало, текущая точка, точки карандаша
    Drawing { tool: Tool, anchor: P, end: P, pts: Vec<P> },
}

/// Подпись, которую сейчас набирают.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub at: P,
    pub text: String,
    pub size: f32,
    pub color: Color,
}

/// Насколько надо сдвинуть мышь, чтобы щелчок стал протягиванием.
const DRAG_THRESHOLD: i32 = 3;
/// Насколько близко к краю рамки начинается ручка.
const HANDLE_REACH: i32 = 8;
const BIG_STEP: i32 = 10;
const MAX_TEXT_LEN: usize = 300;

pub struct Session {
    width: i32,
    height: i32,
    monitors: Vec<Monitor>,
    sel: Option<Rect>,
    drag: Drag,
    tool: Option<Tool>,
    color: Color,
    stroke: f32,
    text_size: f32,
    marks: Marks,
    editing: Option<Edit>,
    shift: bool,
    pointer: Option<Pt>,
    hover: Option<BarButton>,
    preview: bool,
    /// Найденное опасное (координаты кадра) и закрашено ли
    findings: Vec<(Finding, bool)>,
    /// Режим «Опасное»: найденное подсвечено, щелчок закрашивает
    danger: bool,
    detect: Detect,
    hover_finding: Option<usize>,
}

/// Состояние поиска опасного.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detect {
    /// Распознавание текста недоступно (или выключено `--no-detect`)
    Off,
    /// Идёт распознавание
    Searching,
    Done,
}

impl Session {
    pub fn new(width: u32, height: u32) -> Session {
        let all = Rect { x: 0, y: 0, w: width as i32, h: height as i32 };
        Session::with_monitors(width, height, vec![Monitor { rect: all, scale: 100 }])
    }

    pub fn with_monitors(width: u32, height: u32, monitors: Vec<Monitor>) -> Session {
        Session {
            width: width as i32,
            height: height as i32,
            monitors,
            sel: None,
            drag: Drag::None,
            tool: None,
            color: Color::default(),
            stroke: annot::DEFAULT_WIDTH,
            text_size: annot::DEFAULT_TEXT,
            marks: Marks::new(),
            editing: None,
            shift: false,
            pointer: None,
            hover: None,
            preview: false,
            findings: Vec::new(),
            danger: false,
            detect: Detect::Off,
            hover_finding: None,
        }
    }

    // ---------- поиск опасного ----------

    /// Распознавание запущено: режим «Опасное» включён, ждём находок.
    pub fn start_detect(&mut self) {
        self.detect = Detect::Searching;
        self.danger = true;
    }

    pub fn set_findings(&mut self, findings: Vec<Finding>) {
        self.findings = findings.into_iter().map(|f| (f, false)).collect();
        self.detect = Detect::Done;
    }

    pub fn detect(&self) -> Detect {
        self.detect
    }

    pub fn danger(&self) -> bool {
        self.danger
    }

    /// Находки внутри зоны: (номер, находка, закрашена).
    pub fn findings(&self) -> impl Iterator<Item = (usize, &Finding, bool)> {
        let sel = self.sel;
        self.findings
            .iter()
            .enumerate()
            .filter(move |(_, (f, _))| sel.is_some_and(|s| inside(s, f.bounds())))
            .map(|(i, (f, r))| (i, f, *r))
    }

    pub fn hover_finding(&self) -> Option<&Finding> {
        self.hover_finding.and_then(|i| self.findings.get(i)).map(|(f, _)| f)
    }

    /// Прямоугольники закрашенных находок — уходят в снимок вместе с пометками.
    pub fn redactions(&self) -> Vec<Rect> {
        self.findings.iter().filter(|(_, r)| *r).flat_map(|(f, _)| f.rects.iter().copied()).collect()
    }

    fn finding_at(&self, p: Pt) -> Option<usize> {
        if !self.danger {
            return None;
        }
        self.findings().find(|(_, f, _)| f.contains(p.x, p.y)).map(|(i, _, _)| i)
    }

    /// Щелчок по находке — закрасить или снять закраску (с инструментом или без него).
    fn toggle_finding(&mut self, p: Pt) -> bool {
        let tool_ok = matches!(self.tool, None | Some(Tool::Redact));
        match self.finding_at(p) {
            Some(i) if tool_ok => {
                self.findings[i].1 = !self.findings[i].1;
                true
            }
            _ => false,
        }
    }

    fn redact_all(&mut self) -> Outcome {
        let ids: Vec<usize> = self.findings().map(|(i, _, _)| i).collect();
        if ids.is_empty() {
            return Outcome::Nothing;
        }
        for i in ids {
            self.findings[i].1 = true;
        }
        self.danger = true;
        Outcome::Redraw
    }

    // ---------- то, что нужно отрисовке ----------

    pub fn selection(&self) -> Option<Rect> {
        self.sel
    }

    pub fn tool(&self) -> Option<Tool> {
        self.tool
    }

    pub fn color(&self) -> Color {
        self.color
    }

    pub fn stroke(&self) -> f32 {
        self.stroke
    }

    pub fn text_size(&self) -> f32 {
        self.text_size
    }

    pub fn marks(&self) -> &Marks {
        &self.marks
    }

    pub fn editing(&self) -> Option<&Edit> {
        self.editing.as_ref()
    }

    pub fn hover(&self) -> Option<BarButton> {
        self.hover
    }

    pub fn toolbar(&self) -> Option<Bar> {
        let screen = Rect { x: 0, y: 0, w: self.width, h: self.height };
        self.sel.map(|s| toolbar::layout(s, screen, &self.monitors))
    }

    /// Пометка, которую сейчас рисуют или набирают — ещё не в списке.
    pub fn live(&self) -> Option<Mark> {
        if let Some(e) = &self.editing {
            return Some(Mark {
                shape: Shape::Text { at: e.at, text: e.text.clone(), size: e.size },
                color: e.color,
                width: self.stroke,
            });
        }
        let Drag::Drawing { tool, anchor, end, pts } = &self.drag else { return None };
        let shape = match tool {
            Tool::Pen => Shape::Pen(pts.clone()),
            Tool::Line => Shape::Line(*anchor, if self.shift { annot::snap45(*anchor, *end) } else { *end }),
            Tool::Arrow => Shape::Arrow(*anchor, if self.shift { annot::snap45(*anchor, *end) } else { *end }),
            Tool::Rect => Shape::Rect(*anchor, if self.shift { annot::square(*anchor, *end) } else { *end }),
            Tool::Redact => Shape::Redact(*anchor, *end),
            Tool::Text => return None,
        };
        Some(Mark { shape, color: self.color, width: self.stroke })
    }

    /// Превью толщины (или размера подписи) у курсора — после поворота колёсика.
    pub fn preview(&self) -> Option<(Pt, f32, bool)> {
        if !self.preview {
            return None;
        }
        let text = self.editing.is_some() || self.tool == Some(Tool::Text);
        let value = match &self.editing {
            Some(e) => e.size,
            None if text => self.text_size,
            None => self.stroke,
        };
        self.pointer.map(|p| (p, value, text))
    }

    /// Курсор во время текущего действия.
    pub fn cursor(&self) -> Cursor {
        match self.drag {
            Drag::Creating { .. } | Drag::Drawing { .. } | Drag::None => Cursor::Cross,
            Drag::Moving { .. } => Cursor::Move,
            Drag::Resizing { edges, .. } => edges_cursor(edges),
        }
    }

    /// Какой курсор показать над точкой `p`.
    pub fn cursor_at(&self, p: Pt) -> Cursor {
        if !matches!(self.drag, Drag::None) {
            return self.cursor();
        }
        if self.toolbar().is_some_and(|b| b.contains(p)) || self.finding_at(p).is_some() {
            return Cursor::Arrow;
        }
        match self.sel {
            Some(sel) => match hit(sel, p) {
                Some(edges) => edges_cursor(edges),
                None if sel.contains(p) => match self.tool {
                    None => Cursor::Move,
                    Some(Tool::Text) => Cursor::Text,
                    Some(_) => Cursor::Cross,
                },
                None => Cursor::Cross,
            },
            None => Cursor::Cross,
        }
    }

    // ---------- ввод ----------

    fn clamp_pt(&self, p: Pt) -> Pt {
        Pt { x: p.x.clamp(0, self.width), y: p.y.clamp(0, self.height) }
    }

    /// Сдвинуть зону, не выпуская за экран.
    fn place(&self, r: Rect) -> Rect {
        let w = r.w.min(self.width);
        let h = r.h.min(self.height);
        Rect { x: r.x.clamp(0, self.width - w), y: r.y.clamp(0, self.height - h), w, h }
    }

    /// Закончить набор подписи: непустая — в пометки.
    fn commit_edit(&mut self) -> bool {
        match self.editing.take() {
            Some(e) => {
                self.marks.push(Mark {
                    shape: Shape::Text { at: e.at, text: e.text, size: e.size },
                    color: e.color,
                    width: self.stroke,
                });
                true
            }
            None => false,
        }
    }

    /// Итог «готово/сохранить» — с подписью, если её ещё набирают.
    fn finish(&mut self, make: fn(Rect) -> Outcome) -> Outcome {
        self.commit_edit();
        self.sel.map_or(Outcome::Nothing, make)
    }

    pub fn input(&mut self, input: Input) -> Outcome {
        match input {
            Input::Repaint => Outcome::Redraw,
            Input::Shift(on) => {
                let changed = self.shift != on;
                self.shift = on;
                if changed && matches!(self.drag, Drag::Drawing { .. }) {
                    Outcome::Redraw
                } else {
                    Outcome::Nothing
                }
            }
            Input::Down(Button::Right, _) => {
                if self.editing.take().is_some() || matches!(self.drag, Drag::Drawing { .. }) {
                    self.drag = Drag::None;
                    return Outcome::Redraw;
                }
                self.drag = Drag::None;
                if self.sel.take().is_some() {
                    Outcome::Redraw
                } else {
                    Outcome::Cancel
                }
            }
            Input::Down(Button::Left, p) => self.left_down(p),
            Input::Move(p) => {
                self.pointer = Some(p);
                let hover = self.toolbar().and_then(|b| b.hit(p));
                let hf = if hover.is_none() && matches!(self.drag, Drag::None) { self.finding_at(p) } else { None };
                let mut redraw = hover != self.hover || self.preview || hf != self.hover_finding;
                self.hover = hover;
                self.hover_finding = hf;
                self.preview = false;
                if self.drag_to(p) == Outcome::Redraw {
                    redraw = true;
                }
                if redraw { Outcome::Redraw } else { Outcome::Nothing }
            }
            Input::Up(Button::Left, p) => {
                let mut out = self.drag_to(p);
                if let Drag::Drawing { .. } = self.drag {
                    if let Some(m) = self.live() {
                        self.marks.push(m);
                    }
                    out = Outcome::Redraw;
                }
                self.drag = Drag::None;
                out
            }
            Input::Up(Button::Right, _) => Outcome::Nothing,
            Input::DoubleClick(p) => {
                if self.tool.is_some() || self.toolbar().is_some_and(|b| b.contains(p)) {
                    // с инструментом двойной щелчок — это просто ещё один щелчок
                    return self.left_down(p);
                }
                match self.sel {
                    Some(sel) if sel.contains(p) => Outcome::Confirm(sel),
                    _ => Outcome::Nothing,
                }
            }
            Input::Wheel(notches, p) => {
                self.pointer = Some(p);
                self.preview = true;
                if let Some(e) = &mut self.editing {
                    e.size = annot::clamp_text(e.size + notches as f32 * annot::TEXT_STEP);
                } else if self.tool == Some(Tool::Text) {
                    self.text_size = annot::clamp_text(self.text_size + notches as f32 * annot::TEXT_STEP);
                } else {
                    self.stroke = annot::clamp_width(self.stroke + notches as f32);
                }
                Outcome::Redraw
            }
            Input::Char(c) => match &mut self.editing {
                Some(e) if !c.is_control() && e.text.chars().count() < MAX_TEXT_LEN => {
                    e.text.push(c);
                    Outcome::Redraw
                }
                _ => Outcome::Nothing,
            },
            Input::Key { key, shift, ctrl } => self.key(key, shift, ctrl),
        }
    }

    fn left_down(&mut self, p: Pt) -> Outcome {
        let p = self.clamp_pt(p);
        if let Some(bar) = self.toolbar() {
            if bar.contains(p) {
                return match bar.hit(p) {
                    Some(b) => self.press(b),
                    None => Outcome::Nothing,
                };
            }
        }
        if self.editing.is_none() && self.toggle_finding(p) {
            return Outcome::Redraw;
        }
        let committed = self.commit_edit();
        self.drag = match self.sel {
            Some(sel) => match hit(sel, p) {
                Some(edges) => Drag::Resizing { edges, grab: p, start: sel },
                None if sel.contains(p) => match self.tool {
                    None => Drag::Moving { grab: p, start: sel },
                    Some(Tool::Text) => {
                        self.editing = Some(Edit {
                            at: P::new(p.x as f32, p.y as f32),
                            text: String::new(),
                            size: self.text_size,
                            color: self.color,
                        });
                        return Outcome::Redraw;
                    }
                    Some(tool) => {
                        let at = P::new(p.x as f32, p.y as f32);
                        Drag::Drawing { tool, anchor: at, end: at, pts: vec![at] }
                    }
                },
                None => Drag::Creating { anchor: p, started: false },
            },
            None => Drag::Creating { anchor: p, started: false },
        };
        if committed || matches!(self.drag, Drag::Drawing { .. }) {
            Outcome::Redraw
        } else {
            Outcome::Nothing
        }
    }

    /// Нажата кнопка панели.
    fn press(&mut self, b: BarButton) -> Outcome {
        match b {
            BarButton::Tool(t) => {
                self.commit_edit();
                self.tool = if self.tool == Some(t) { None } else { Some(t) };
                Outcome::Redraw
            }
            BarButton::Color(c) => {
                self.color = c;
                if let Some(e) = &mut self.editing {
                    e.color = c;
                }
                Outcome::Redraw
            }
            BarButton::Undo => {
                if self.editing.take().is_none() {
                    self.marks.undo();
                }
                Outcome::Redraw
            }
            BarButton::Save => self.finish(Outcome::SaveAs),
            BarButton::Danger => {
                self.danger = !self.danger;
                Outcome::Redraw
            }
            BarButton::RedactAll => self.redact_all(),
            BarButton::Copy => self.finish(Outcome::Confirm),
            BarButton::Close => Outcome::Cancel,
        }
    }

    fn drag_to(&mut self, p: Pt) -> Outcome {
        let p = self.clamp_pt(p);
        match &mut self.drag {
            Drag::None => Outcome::Nothing,
            Drag::Drawing { end, pts, tool, .. } => {
                let at = P::new(p.x as f32, p.y as f32);
                *end = at;
                if *tool == Tool::Pen && pts.last() != Some(&at) {
                    pts.push(at);
                }
                Outcome::Redraw
            }
            Drag::Creating { anchor, started } => {
                let anchor = *anchor;
                let far = (p.x - anchor.x).abs().max((p.y - anchor.y).abs()) >= DRAG_THRESHOLD;
                if !*started && !far {
                    return Outcome::Nothing;
                }
                *started = true;
                self.sel = Some(Rect::from_edges(anchor.x, anchor.y, p.x, p.y));
                Outcome::Redraw
            }
            Drag::Moving { grab, start } => {
                let moved = Rect { x: start.x + p.x - grab.x, y: start.y + p.y - grab.y, ..*start };
                self.sel = Some(self.place(moved));
                Outcome::Redraw
            }
            Drag::Resizing { edges, grab, start } => {
                let (edges, start) = (*edges, *start);
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
        // Набор подписи: Enter и Esc относятся к подписи, а не ко всему снимку.
        if let Some(e) = &mut self.editing {
            match key {
                Key::Enter => {
                    self.commit_edit();
                    return Outcome::Redraw;
                }
                Key::Esc => {
                    self.editing = None;
                    return Outcome::Redraw;
                }
                Key::Backspace => {
                    e.text.pop();
                    return Outcome::Redraw;
                }
                Key::Letter('C' | 'S') if ctrl => {} // готово/сохранить — вместе с подписью
                _ => return Outcome::Nothing,     // буквы идут через Char
            }
        }
        match key {
            Key::Esc => return Outcome::Cancel,
            Key::Enter => return self.finish(Outcome::Confirm),
            Key::Backspace => return Outcome::Nothing,
            Key::Letter(c) if ctrl => {
                return match c {
                    'C' => self.finish(Outcome::Confirm),
                    'S' => self.finish(Outcome::Save),
                    'Z' if shift => self.redo(),
                    'Z' => self.undo(),
                    'Y' => self.redo(),
                    _ => Outcome::Nothing,
                }
            }
            Key::Letter('C') => {
                self.color = self.color.other();
                return Outcome::Redraw;
            }
            Key::Letter('D') if shift => return self.redact_all(),
            Key::Letter('D') => {
                self.danger = !self.danger;
                return Outcome::Redraw;
            }
            Key::Letter(c) => {
                return match Tool::from_key(c) {
                    Some(t) => {
                        self.tool = if self.tool == Some(t) { None } else { Some(t) };
                        Outcome::Redraw
                    }
                    None => Outcome::Nothing,
                }
            }
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

    fn undo(&mut self) -> Outcome {
        if self.marks.undo() { Outcome::Redraw } else { Outcome::Nothing }
    }

    fn redo(&mut self) -> Outcome {
        if self.marks.redo() { Outcome::Redraw } else { Outcome::Nothing }
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

/// Прямоугольник `inner` целиком внутри `outer`.
fn inside(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x && inner.y >= outer.y && inner.right() <= outer.right() && inner.bottom() <= outer.bottom()
}
