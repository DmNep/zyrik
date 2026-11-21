//! T019: выделение области мышью и клавиатурой.

use zyrik::session::{Button, Cursor, Input, Key, Outcome, Pt, Rect, Session};

const W: i32 = 1000;
const H: i32 = 800;

fn s() -> Session {
    Session::new(W as u32, H as u32)
}

fn pt(x: i32, y: i32) -> Pt {
    Pt { x, y }
}

fn down(s: &mut Session, x: i32, y: i32) -> Outcome {
    s.input(Input::Down(Button::Left, pt(x, y)))
}
fn mv(s: &mut Session, x: i32, y: i32) -> Outcome {
    s.input(Input::Move(pt(x, y)))
}
fn up(s: &mut Session, x: i32, y: i32) -> Outcome {
    s.input(Input::Up(Button::Left, pt(x, y)))
}
fn drag(s: &mut Session, from: (i32, i32), to: (i32, i32)) {
    down(s, from.0, from.1);
    mv(s, (from.0 + to.0) / 2, (from.1 + to.1) / 2);
    mv(s, to.0, to.1);
    up(s, to.0, to.1);
}
fn key(s: &mut Session, k: Key) -> Outcome {
    s.input(Input::Key { key: k, shift: false, ctrl: false })
}
fn key_mod(s: &mut Session, k: Key, shift: bool, ctrl: bool) -> Outcome {
    s.input(Input::Key { key: k, shift, ctrl })
}

fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect { x, y, w, h }
}

#[test]
fn starts_empty_with_crosshair() {
    let s = s();
    assert_eq!(s.selection(), None);
    assert_eq!(s.cursor(), Cursor::Cross);
}

#[test]
fn drag_creates_selection_in_any_direction() {
    let mut a = s();
    drag(&mut a, (100, 200), (300, 260));
    assert_eq!(a.selection(), Some(r(100, 200, 200, 60)));

    let mut b = s();
    drag(&mut b, (300, 260), (100, 200));
    assert_eq!(b.selection(), Some(r(100, 200, 200, 60)));
}

#[test]
fn selection_is_visible_while_dragging() {
    let mut s = s();
    down(&mut s, 10, 10);
    assert_eq!(mv(&mut s, 50, 40), Outcome::Redraw);
    assert_eq!(s.selection(), Some(r(10, 10, 40, 30)));
}

#[test]
fn click_without_drag_creates_nothing() {
    let mut s = s();
    down(&mut s, 100, 100);
    mv(&mut s, 101, 101); // дрожание руки
    up(&mut s, 101, 101);
    assert_eq!(s.selection(), None);
    assert_eq!(key(&mut s, Key::Enter), Outcome::Nothing, "пустой снимок не делаем");
}

#[test]
fn click_outside_keeps_old_selection_drag_outside_replaces_it() {
    let mut s = s();
    drag(&mut s, (100, 100), (200, 200));
    down(&mut s, 500, 500);
    up(&mut s, 500, 500);
    assert_eq!(s.selection(), Some(r(100, 100, 100, 100)));
    drag(&mut s, (500, 500), (600, 550));
    assert_eq!(s.selection(), Some(r(500, 500, 100, 50)));
}

#[test]
fn drag_is_clamped_to_screen() {
    let mut s = s();
    drag(&mut s, (900, 700), (5000, -300));
    assert_eq!(s.selection(), Some(r(900, 0, 100, 700)));
}

#[test]
fn moving_by_dragging_inside() {
    let mut s = s();
    drag(&mut s, (100, 100), (300, 200));
    assert_eq!(s.cursor_at(pt(200, 150)), Cursor::Move);
    drag(&mut s, (200, 150), (250, 170));
    assert_eq!(s.selection(), Some(r(150, 120, 200, 100)));
    // за край экрана не уезжает, размер сохраняется
    drag(&mut s, (200, 150), (-500, -500));
    assert_eq!(s.selection(), Some(r(0, 0, 200, 100)));
}

#[test]
fn resizing_by_handles() {
    let mut s = s();
    drag(&mut s, (100, 100), (300, 200));
    assert_eq!(s.cursor_at(pt(300, 200)), Cursor::ResizeNwse);
    assert_eq!(s.cursor_at(pt(300, 100)), Cursor::ResizeNesw);
    assert_eq!(s.cursor_at(pt(200, 100)), Cursor::ResizeNs);
    assert_eq!(s.cursor_at(pt(100, 150)), Cursor::ResizeWe);
    assert_eq!(s.cursor_at(pt(600, 600)), Cursor::Cross);

    // правый нижний угол
    drag(&mut s, (300, 200), (400, 260));
    assert_eq!(s.selection(), Some(r(100, 100, 300, 160)));
    // верхний край — меняется только высота
    drag(&mut s, (250, 100), (999, 80));
    assert_eq!(s.selection(), Some(r(100, 80, 300, 180)));
    // левый край перетащили за правый — рамка выворачивается, а не ломается
    drag(&mut s, (100, 150), (500, 150));
    assert_eq!(s.selection(), Some(r(400, 80, 100, 180)));
}

#[test]
fn resize_never_collapses_to_zero() {
    let mut s = s();
    drag(&mut s, (100, 100), (300, 200));
    drag(&mut s, (300, 150), (100, 150)); // правый край на левый
    let sel = s.selection().unwrap();
    assert!(sel.w >= 1 && sel.h >= 1, "{sel:?}");
}

#[test]
fn arrows_move_by_one_shift_by_ten() {
    let mut s = s();
    drag(&mut s, (100, 100), (200, 200));
    key(&mut s, Key::Right);
    key(&mut s, Key::Down);
    assert_eq!(s.selection(), Some(r(101, 101, 100, 100)));
    key_mod(&mut s, Key::Left, true, false);
    key_mod(&mut s, Key::Up, true, false);
    assert_eq!(s.selection(), Some(r(91, 91, 100, 100)));
    for _ in 0..20 {
        key_mod(&mut s, Key::Left, true, false);
    }
    assert_eq!(s.selection().unwrap().x, 0, "не за край экрана");
}

#[test]
fn ctrl_arrows_resize() {
    let mut s = s();
    drag(&mut s, (100, 100), (200, 200));
    key_mod(&mut s, Key::Right, false, true);
    key_mod(&mut s, Key::Down, true, true);
    assert_eq!(s.selection(), Some(r(100, 100, 101, 110)));
    for _ in 0..30 {
        key_mod(&mut s, Key::Left, true, true);
    }
    assert_eq!(s.selection(), Some(r(100, 100, 1, 110)));
}

#[test]
fn arrows_without_selection_do_nothing() {
    let mut s = s();
    assert_eq!(key(&mut s, Key::Left), Outcome::Nothing);
    assert_eq!(s.selection(), None);
}

#[test]
fn enter_and_ctrl_c_confirm() {
    let mut s = s();
    drag(&mut s, (10, 20), (110, 70));
    assert_eq!(key(&mut s, Key::Enter), Outcome::Confirm(r(10, 20, 100, 50)));
    assert_eq!(key_mod(&mut s, Key::C, false, true), Outcome::Confirm(r(10, 20, 100, 50)));
    assert_eq!(key(&mut s, Key::C), Outcome::Nothing, "просто C — не копирование");
}

#[test]
fn esc_cancels() {
    let mut s = s();
    assert_eq!(key(&mut s, Key::Esc), Outcome::Cancel);
    drag(&mut s, (10, 20), (110, 70));
    assert_eq!(key(&mut s, Key::Esc), Outcome::Cancel);
}

#[test]
fn right_click_clears_selection_then_cancels() {
    let mut s = s();
    drag(&mut s, (10, 20), (110, 70));
    assert_eq!(s.input(Input::Down(Button::Right, pt(50, 50))), Outcome::Redraw);
    assert_eq!(s.selection(), None);
    assert_eq!(s.input(Input::Down(Button::Right, pt(50, 50))), Outcome::Cancel);
}

#[test]
fn double_click_inside_confirms() {
    let mut s = s();
    drag(&mut s, (10, 20), (110, 70));
    assert_eq!(s.input(Input::DoubleClick(pt(50, 50))), Outcome::Confirm(r(10, 20, 100, 50)));
    assert_eq!(s.input(Input::DoubleClick(pt(500, 500))), Outcome::Nothing);
}
