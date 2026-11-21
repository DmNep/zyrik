//! Свой выбор папки для «Сохранить как…» (дискета): панель внутри окна выделения.
//!
//! Системный диалог Windows записывает в реестр имя программы и последнюю папку
//! (`ComDlg32\LastVisitedPidlMRU`) — флагами это не выключается. Поэтому папку выбираем сами:
//! только чтение списков папок через `std::fs`, ни реестра, ни «Недавних», ни пробных файлов.
//! Папки не создаёт и файлы не перезаписывает: сохранение — `export::save_unique` в выбранную папку.
//!
//! Мышь: щелчок — выбрать строку, двойной — открыть, колёсико — листать.
//! Клавиши: ↑↓ — по списку, Enter/→ — открыть, Backspace/← — вверх, буква — к папке на эту букву,
//! Ctrl+S или Ctrl+Enter — сохранить сюда, Esc или правая кнопка — назад к выделению.

use std::io;
use std::path::{Path, PathBuf};

use crate::platform::{Monitor, Place, PlaceKind};
use crate::session::{Button, Input, Key, Pt, Rect};

/// Строка списка.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// «..» — на уровень выше
    Up,
    /// Известная папка или диск — на верхнем уровне
    Place(Place),
    /// Вложенная папка: имя
    Folder(String),
}

/// Что сказать человеку под списком.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Ctrl+S не смог сохранить в последнюю папку
    Unavailable(PathBuf, String),
    /// Папку не открыть (нет доступа, нет диска)
    CantOpen(String),
    /// Не удалось записать файл в выбранную папку
    SaveFailed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Номер строки в списке (не на экране)
    Row(usize),
    Save,
    Back,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickOutcome {
    Nothing,
    Redraw,
    /// Сохранить в эту папку
    Save(PathBuf),
    /// Назад к выделению, ничего не сохраняя
    Close,
}

/// Где что нарисовано (физические пиксели кадра).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub panel: Rect,
    pub scale: u32,
    pub pad: i32,
    pub title_y: i32,
    pub path_y: i32,
    /// Список: первая строка, высота строки, сколько строк видно
    pub list: Rect,
    pub row_h: i32,
    pub rows: usize,
    pub file_y: i32,
    pub status_y: i32,
    pub save: Rect,
    pub back: Rect,
    pub hint_y: i32,
    pub title_size: f32,
    pub text_size: f32,
    pub small_size: f32,
}

const WIDTH: i32 = 600;
const ROW: i32 = 26;
const MIN_ROWS: i32 = 4;
const MAX_ROWS: i32 = 12;

fn sc(v: i32, scale: u32) -> i32 {
    (v * scale as i32 + 50) / 100
}

impl Layout {
    /// Панель по центру монитора `m`; на маленьком мониторе строк меньше.
    pub fn new(m: Monitor) -> Layout {
        let scale = m.scale.max(100);
        let s = |v| sc(v, scale);
        let pad = s(14);
        let row_h = s(ROW);
        // всё, кроме списка: заголовок, путь, имя файла, сообщение, кнопки, подсказка, отступы
        let fixed = pad + s(30) + s(24) + s(6) + s(8) + s(22) + s(22) + s(34) + s(8) + s(20) + pad;
        let fit = (m.rect.h * 8 / 10 - fixed) / row_h;
        let rows = fit.clamp(MIN_ROWS, MAX_ROWS);
        let w = s(WIDTH).min(m.rect.w);
        let h = fixed + rows * row_h;
        let x = m.rect.x + (m.rect.w - w) / 2;
        let y = m.rect.y + ((m.rect.h - h) / 2).max(0);
        let title_y = y + pad;
        let path_y = title_y + s(30);
        let list = Rect { x: x + pad, y: path_y + s(24) + s(6), w: w - 2 * pad, h: rows * row_h };
        let file_y = list.bottom() + s(8);
        let status_y = file_y + s(22);
        let by = status_y + s(22);
        let bw = s(180);
        let back = Rect { x: x + w - pad - bw, y: by, w: bw, h: s(34) };
        let save = Rect { x: back.x - s(10) - bw, ..back };
        Layout {
            panel: Rect { x, y, w, h },
            scale,
            pad,
            title_y,
            path_y,
            list,
            row_h,
            rows: rows as usize,
            file_y,
            status_y,
            save,
            back,
            hint_y: by + s(34) + s(8),
            title_size: 17.0 * scale as f32 / 100.0,
            text_size: 15.0 * scale as f32 / 100.0,
            small_size: 12.5 * scale as f32 / 100.0,
        }
    }

    pub fn row_rect(&self, visible: usize) -> Rect {
        Rect { x: self.list.x, y: self.list.y + visible as i32 * self.row_h, w: self.list.w, h: self.row_h }
    }
}

pub struct Picker {
    /// `None` — верхний уровень: известные папки и диски
    dir: Option<PathBuf>,
    places: Vec<Place>,
    entries: Vec<Entry>,
    cur: usize,
    /// Первая видимая строка
    top: usize,
    status: Option<Status>,
    /// Имя файла, которое получит снимок (без `_2`, если такое уже есть)
    file_name: String,
    hover: Option<Hit>,
    layout: Layout,
}

/// Вложенные папки, по алфавиту без учёта регистра. Скрытые и системные не показываем.
pub fn list_dirs(dir: &Path) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let Ok(e) = e else { continue };
        let Ok(ft) = e.file_type() else { continue };
        let is_dir = ft.is_dir() || (ft.is_symlink() && e.path().is_dir());
        if !is_dir || hidden(&e) {
            continue;
        }
        if let Ok(name) = e.file_name().into_string() {
            out.push(name);
        }
    }
    out.sort_by_cached_key(|n| n.to_lowercase());
    Ok(out)
}

#[cfg(windows)]
fn hidden(e: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    e.metadata().map(|m| m.file_attributes() & (HIDDEN | SYSTEM) != 0).unwrap_or(true)
}

#[cfg(not(windows))]
fn hidden(e: &std::fs::DirEntry) -> bool {
    e.file_name().to_string_lossy().starts_with('.')
}

fn os_error(e: &io::Error) -> String {
    e.to_string()
}

impl Picker {
    /// Открыть в папке `start`; если её нет — в ближайшей существующей папке выше, иначе наверху.
    pub fn open(start: Option<&Path>, places: Vec<Place>, file_name: String, monitor: Monitor) -> Picker {
        let mut p = Picker {
            dir: None,
            places,
            entries: Vec::new(),
            cur: 0,
            top: 0,
            status: None,
            file_name,
            hover: None,
            layout: Layout::new(monitor),
        };
        p.show_top(None);
        let mut at = start;
        while let Some(dir) = at {
            if p.go(dir.to_path_buf(), None) {
                break;
            }
            at = dir.parent().filter(|d| !d.as_os_str().is_empty());
        }
        p.status = None;
        p
    }

    // ---------- то, что нужно отрисовке ----------

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn current(&self) -> usize {
        self.cur
    }

    pub fn top(&self) -> usize {
        self.top
    }

    pub fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    pub fn set_status(&mut self, s: Status) {
        self.status = Some(s);
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn hover(&self) -> Option<Hit> {
        self.hover
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Что под точкой `p`.
    pub fn hit(&self, p: Pt) -> Option<Hit> {
        let l = &self.layout;
        if l.save.contains(p) {
            return Some(Hit::Save);
        }
        if l.back.contains(p) {
            return Some(Hit::Back);
        }
        if l.list.contains(p) {
            let i = self.top + ((p.y - l.list.y) / l.row_h) as usize;
            return (i < self.entries.len()).then_some(Hit::Row(i));
        }
        None
    }

    // ---------- переходы ----------

    /// Верхний уровень; `select` — какую папку подсветить.
    fn show_top(&mut self, select: Option<&Path>) {
        self.dir = None;
        self.entries = self.places.iter().cloned().map(Entry::Place).collect();
        self.cur = select
            .and_then(|s| self.entries.iter().position(|e| matches!(e, Entry::Place(p) if p.path == s)))
            .unwrap_or(0);
        self.top = 0;
        self.status = None;
        self.reveal();
    }

    /// Открыть папку. `select` — имя вложенной папки, которую подсветить (откуда пришли).
    fn go(&mut self, dir: PathBuf, select: Option<&str>) -> bool {
        match list_dirs(&dir) {
            Ok(names) => {
                let first = usize::from(!names.is_empty());
                self.cur = select
                    .and_then(|s| names.iter().position(|n| n == s))
                    .map_or(first, |i| i + 1);
                self.entries = std::iter::once(Entry::Up).chain(names.into_iter().map(Entry::Folder)).collect();
                self.dir = Some(dir);
                self.top = 0;
                self.status = None;
                self.reveal();
                true
            }
            Err(e) => {
                self.status = Some(Status::CantOpen(os_error(&e)));
                false
            }
        }
    }

    fn up(&mut self) -> PickOutcome {
        let Some(dir) = self.dir.clone() else { return PickOutcome::Nothing };
        let name = dir.file_name().and_then(|n| n.to_str()).map(str::to_string);
        match dir.parent().filter(|p| !p.as_os_str().is_empty()) {
            Some(parent) if name.is_some() => {
                if !self.go(parent.to_path_buf(), name.as_deref()) {
                    self.show_top(Some(&dir));
                }
            }
            // корень диска — наверх, к списку дисков
            _ => self.show_top(Some(&dir)),
        }
        PickOutcome::Redraw
    }

    fn open_entry(&mut self, i: usize) -> PickOutcome {
        match self.entries.get(i).cloned() {
            Some(Entry::Up) => self.up(),
            Some(Entry::Place(p)) => {
                self.cur = i;
                self.go(p.path, None);
                PickOutcome::Redraw
            }
            Some(Entry::Folder(name)) => {
                self.cur = i;
                if let Some(dir) = self.dir.clone() {
                    self.go(dir.join(name), None);
                }
                PickOutcome::Redraw
            }
            None => PickOutcome::Nothing,
        }
    }

    fn save_here(&self) -> PickOutcome {
        match &self.dir {
            Some(d) => PickOutcome::Save(d.clone()),
            None => PickOutcome::Nothing,
        }
    }

    /// Прокрутить так, чтобы подсвеченная строка была видна.
    fn reveal(&mut self) {
        let rows = self.layout.rows;
        if self.cur < self.top {
            self.top = self.cur;
        } else if self.cur >= self.top + rows {
            self.top = self.cur + 1 - rows;
        }
    }

    fn max_top(&self) -> usize {
        self.entries.len().saturating_sub(self.layout.rows)
    }

    fn select(&mut self, i: usize) -> PickOutcome {
        let i = i.min(self.entries.len().saturating_sub(1));
        if i == self.cur {
            return PickOutcome::Nothing;
        }
        self.cur = i;
        self.reveal();
        PickOutcome::Redraw
    }

    /// К следующей строке, чьё название начинается на `c` (по кругу).
    fn jump(&mut self, c: char) -> PickOutcome {
        let want: String = c.to_lowercase().collect();
        let n = self.entries.len();
        for k in 1..=n {
            let i = (self.cur + k) % n;
            let name = match &self.entries[i] {
                Entry::Folder(name) => name.as_str(),
                _ => continue,
            };
            if name.to_lowercase().starts_with(&want) {
                return self.select(i);
            }
        }
        PickOutcome::Nothing
    }

    // ---------- ввод ----------

    pub fn input(&mut self, input: Input) -> PickOutcome {
        match input {
            Input::Repaint => PickOutcome::Redraw,
            Input::Move(p) => {
                let h = self.hit(p);
                if h != self.hover {
                    self.hover = h;
                    PickOutcome::Redraw
                } else {
                    PickOutcome::Nothing
                }
            }
            Input::Down(Button::Right, _) => PickOutcome::Close,
            Input::Down(Button::Left, p) => match self.hit(p) {
                Some(Hit::Row(i)) => self.select(i),
                Some(Hit::Save) => self.save_here(),
                Some(Hit::Back) => PickOutcome::Close,
                None => PickOutcome::Nothing,
            },
            Input::DoubleClick(p) => match self.hit(p) {
                Some(Hit::Row(i)) => self.open_entry(i),
                _ => self.input(Input::Down(Button::Left, p)),
            },
            Input::Wheel(notches, _) => {
                let top = (self.top as i64 - notches as i64 * 3).clamp(0, self.max_top() as i64) as usize;
                if top == self.top {
                    return PickOutcome::Nothing;
                }
                self.top = top;
                PickOutcome::Redraw
            }
            Input::Char(c) if !c.is_control() && !c.is_whitespace() => self.jump(c),
            Input::Key { key, shift, ctrl } => {
                let page = if shift { self.layout.rows.saturating_sub(1).max(1) } else { 1 };
                match key {
                    Key::Esc => PickOutcome::Close,
                    Key::Enter | Key::Letter('S') if ctrl => self.save_here(),
                    Key::Enter | Key::Right => self.open_entry(self.cur),
                    Key::Backspace | Key::Left => self.up(),
                    Key::Up => self.select(self.cur.saturating_sub(page)),
                    Key::Down => self.select(self.cur + page),
                    _ => PickOutcome::Nothing,
                }
            }
            _ => PickOutcome::Nothing,
        }
    }
}

/// Монитор для панели — тот же, где панель инструментов.
pub fn monitor_for(sel: Rect, screen: Rect, monitors: &[Monitor]) -> Monitor {
    crate::toolbar::monitor_of(sel, screen, monitors)
}

impl PlaceKind {
    pub fn is_drive(self) -> bool {
        self == PlaceKind::Drive
    }
}
