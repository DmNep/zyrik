//! Свой выбор папки (дискета): навигация, клавиши, мышь — на настоящих временных папках.

use std::path::{Path, PathBuf};

use zyrik::picker::{list_dirs, Entry, Hit, PickOutcome, Picker, Status};
use zyrik::platform::{Monitor, Place, PlaceKind};
use zyrik::session::{Button, Input, Key, Pt, Rect};

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zyrik-picker-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn monitor() -> Monitor {
    Monitor { rect: Rect { x: 0, y: 0, w: 1920, h: 1080 }, scale: 100 }
}

fn open(start: Option<&Path>, places: Vec<Place>) -> Picker {
    Picker::open(start, places, "Зырик_2026-11-21_09-05-07.png".into(), monitor())
}

fn k(key: Key) -> Input {
    Input::Key { key, shift: false, ctrl: false }
}

fn ctrl(key: Key) -> Input {
    Input::Key { key, shift: false, ctrl: true }
}

fn names(p: &Picker) -> Vec<String> {
    p.entries()
        .iter()
        .map(|e| match e {
            Entry::Up => "..".into(),
            Entry::Place(pl) => pl.path.display().to_string(),
            Entry::Folder(n) => n.clone(),
        })
        .collect()
}

/// Корень файловой системы, на которой лежит временная папка (`C:\` или `/`).
fn root() -> PathBuf {
    std::env::temp_dir().ancestors().last().unwrap().to_path_buf()
}

#[test]
fn lists_only_folders_sorted_ignoring_case() {
    let d = temp_dir("list");
    for n in ["бета", "Альфа", "zeta", "Alpha"] {
        std::fs::create_dir(d.join(n)).unwrap();
    }
    std::fs::write(d.join("файл.txt"), b"x").unwrap();
    assert_eq!(list_dirs(&d).unwrap(), ["Alpha", "zeta", "Альфа", "бета"]);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn opens_in_start_folder_with_first_subfolder_highlighted() {
    let d = temp_dir("start");
    std::fs::create_dir(d.join("a")).unwrap();
    std::fs::create_dir(d.join("b")).unwrap();
    let p = open(Some(&d), vec![]);
    assert_eq!(p.dir(), Some(d.as_path()));
    assert_eq!(names(&p), ["..", "a", "b"]);
    assert_eq!(p.current(), 1, "подсвечена первая папка, не «..»");
    assert_eq!(p.status(), None);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn missing_start_falls_back_to_nearest_existing_parent() {
    let d = temp_dir("gone");
    let p = open(Some(&d.join("нет").join("и тут нет")), vec![]);
    assert_eq!(p.dir(), Some(d.as_path()));
    assert_eq!(p.status(), None);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn no_start_shows_places_and_drives_and_save_is_disabled() {
    let places = vec![
        Place { kind: PlaceKind::Pictures, path: PathBuf::from("/нет/Изображения") },
        Place { kind: PlaceKind::Drive, path: root() },
    ];
    let mut p = open(None, places.clone());
    assert_eq!(p.dir(), None);
    assert_eq!(p.entries(), &[Entry::Place(places[0].clone()), Entry::Place(places[1].clone())]);
    assert_eq!(p.input(ctrl(Key::Letter('S'))), PickOutcome::Nothing, "наверху сохранять некуда");
    let save = p.layout().save;
    assert_eq!(p.input(Input::Down(Button::Left, Pt { x: save.x + 2, y: save.y + 2 })), PickOutcome::Nothing);
}

#[test]
fn enter_opens_backspace_goes_up_and_highlights_where_we_came_from() {
    let d = temp_dir("nav");
    for n in ["a", "b", "c"] {
        std::fs::create_dir(d.join(n)).unwrap();
    }
    std::fs::create_dir(d.join("b").join("inner")).unwrap();
    let mut p = open(Some(&d), vec![]);
    assert_eq!(p.input(k(Key::Down)), PickOutcome::Redraw);
    assert_eq!(p.current(), 2);
    assert_eq!(p.input(k(Key::Enter)), PickOutcome::Redraw);
    assert_eq!(p.dir(), Some(d.join("b").as_path()));
    assert_eq!(names(&p), ["..", "inner"]);
    p.input(k(Key::Backspace));
    assert_eq!(p.dir(), Some(d.as_path()));
    assert_eq!(p.current(), 2, "подсвечена «b» — откуда пришли");
    p.input(k(Key::Right));
    p.input(k(Key::Left));
    assert_eq!(p.dir(), Some(d.as_path()), "← и → — то же, что Backspace и Enter");
    // «..» тоже ведёт наверх
    p.input(k(Key::Up));
    p.input(k(Key::Up));
    assert_eq!(p.current(), 0);
    p.input(k(Key::Enter));
    assert_eq!(p.dir(), d.parent());
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn up_from_drive_root_goes_to_places_with_that_drive_highlighted() {
    let r = root();
    let places = vec![
        Place { kind: PlaceKind::Pictures, path: std::env::temp_dir() },
        Place { kind: PlaceKind::Drive, path: r.clone() },
    ];
    let mut p = open(Some(&r), places);
    assert_eq!(p.dir(), Some(r.as_path()));
    p.input(k(Key::Backspace));
    assert_eq!(p.dir(), None);
    assert_eq!(p.current(), 1);
    assert_eq!(p.input(k(Key::Backspace)), PickOutcome::Nothing, "выше некуда");
    p.input(k(Key::Enter));
    assert_eq!(p.dir(), Some(r.as_path()), "Enter на диске открывает его");
}

#[test]
fn save_here_with_ctrl_s_ctrl_enter_or_button() {
    let d = temp_dir("save");
    let mut p = open(Some(&d), vec![]);
    assert_eq!(p.input(ctrl(Key::Letter('S'))), PickOutcome::Save(d.clone()));
    assert_eq!(p.input(ctrl(Key::Enter)), PickOutcome::Save(d.clone()));
    let b = p.layout().save;
    assert_eq!(p.hit(Pt { x: b.x + 3, y: b.y + 3 }), Some(Hit::Save));
    assert_eq!(p.input(Input::Down(Button::Left, Pt { x: b.x + 3, y: b.y + 3 })), PickOutcome::Save(d.clone()));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn esc_right_click_and_back_button_close() {
    let d = temp_dir("close");
    let mut p = open(Some(&d), vec![]);
    assert_eq!(p.input(k(Key::Esc)), PickOutcome::Close);
    assert_eq!(p.input(Input::Down(Button::Right, Pt { x: 0, y: 0 })), PickOutcome::Close);
    let b = p.layout().back;
    assert_eq!(p.input(Input::Down(Button::Left, Pt { x: b.x + 3, y: b.y + 3 })), PickOutcome::Close);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn click_selects_double_click_opens() {
    let d = temp_dir("mouse");
    for n in ["a", "b"] {
        std::fs::create_dir(d.join(n)).unwrap();
    }
    let mut p = open(Some(&d), vec![]);
    let row = p.layout().row_rect(2);
    let at = Pt { x: row.x + 50, y: row.y + 5 };
    assert_eq!(p.hit(at), Some(Hit::Row(2)));
    assert_eq!(p.input(Input::Move(at)), PickOutcome::Redraw);
    assert_eq!(p.hover(), Some(Hit::Row(2)));
    assert_eq!(p.input(Input::Down(Button::Left, at)), PickOutcome::Redraw);
    assert_eq!(p.current(), 2);
    assert_eq!(p.dir(), Some(d.as_path()), "одиночный щелчок только выбирает");
    p.input(Input::DoubleClick(at));
    assert_eq!(p.dir(), Some(d.join("b").as_path()));
    // мимо строк — ничего
    let below = p.layout().row_rect(5);
    assert_eq!(p.input(Input::Down(Button::Left, Pt { x: below.x + 5, y: below.y + 5 })), PickOutcome::Nothing);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn letter_jumps_to_folder_ignoring_case_and_layout() {
    let d = temp_dir("jump");
    for n in ["Документы", "загрузки", "Здесь", "music"] {
        std::fs::create_dir(d.join(n)).unwrap();
    }
    let mut p = open(Some(&d), vec![]);
    assert_eq!(names(&p), ["..", "music", "Документы", "загрузки", "Здесь"]);
    p.input(Input::Char('з'));
    assert_eq!(p.current(), 3);
    p.input(Input::Char('З'));
    assert_eq!(p.current(), 4, "повтор — к следующей на эту букву");
    p.input(Input::Char('з'));
    assert_eq!(p.current(), 3, "по кругу");
    p.input(Input::Char('M'));
    assert_eq!(p.current(), 1);
    assert_eq!(p.input(Input::Char('ж')), PickOutcome::Nothing);
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn long_lists_scroll_with_wheel_and_follow_keyboard() {
    let d = temp_dir("scroll");
    for i in 0..40 {
        std::fs::create_dir(d.join(format!("p{i:02}"))).unwrap();
    }
    let mut p = open(Some(&d), vec![]);
    let rows = p.layout().rows;
    assert_eq!(p.top(), 0);
    p.input(Input::Wheel(-2, Pt { x: 0, y: 0 }));
    assert_eq!(p.top(), 6, "щелчок колёсика — три строки");
    p.input(Input::Wheel(-100, Pt { x: 0, y: 0 }));
    assert_eq!(p.top(), 41 - rows, "дальше конца не листается");
    p.input(Input::Wheel(100, Pt { x: 0, y: 0 }));
    assert_eq!(p.top(), 0);
    for _ in 0..rows + 3 {
        p.input(k(Key::Down));
    }
    assert_eq!(p.current(), rows + 4);
    assert!(p.current() >= p.top() && p.current() < p.top() + rows, "подсвеченная строка видна");
    p.input(Input::Key { key: Key::Down, shift: true, ctrl: false });
    assert_eq!(p.current(), 2 * rows + 3, "Shift — на страницу");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn folder_that_vanished_shows_error_and_stays() {
    let d = temp_dir("vanish");
    std::fs::create_dir(d.join("x")).unwrap();
    let mut p = open(Some(&d), vec![]);
    std::fs::remove_dir(d.join("x")).unwrap();
    p.input(k(Key::Enter));
    assert_eq!(p.dir(), Some(d.as_path()));
    assert!(matches!(p.status(), Some(Status::CantOpen(_))));
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn panel_is_centered_on_the_monitor_and_scaled() {
    let m = Monitor { rect: Rect { x: 1920, y: 0, w: 2560, h: 1440 }, scale: 150 };
    let p = Picker::open(None, vec![], "x.png".into(), m);
    let l = p.layout();
    assert_eq!(l.panel.w, 900, "600 × 1,5");
    assert_eq!(l.panel.x + l.panel.w / 2, 1920 + 1280, "по центру своего монитора");
    assert!(l.panel.y >= 0 && l.panel.bottom() <= 1440);
    assert!(l.save.bottom() <= l.panel.bottom() && l.back.right() <= l.panel.right());
    // маленький экран — меньше строк, но не меньше четырёх
    let small = Picker::open(None, vec![], "x.png".into(), Monitor { rect: Rect { x: 0, y: 0, w: 800, h: 480 }, scale: 100 });
    assert!(small.layout().rows >= 4 && small.layout().rows < l.rows);
}
