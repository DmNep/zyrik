//! История 4 в режиме выделения: находки, щелчок закрашивает, «Закрасить всё», ручная закраска,
//! и SC-005 — из закрашенного места исходные пиксели не восстановить.

use zyrik::annot::render::bake;
use zyrik::annot::{Shape, Tool, REDACT};
use zyrik::image::Image;
use zyrik::redact::{Category, Finding};
use zyrik::session::{Button, Detect, Input, Key, Outcome, Pt, Rect, Session};

fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect { x, y, w, h }
}

fn key(s: &mut Session, c: char, shift: bool) -> Outcome {
    s.input(Input::Key { key: Key::Letter(c), shift, ctrl: false })
}

fn click(s: &mut Session, x: i32, y: i32) -> Outcome {
    let o = s.input(Input::Down(Button::Left, Pt { x, y }));
    s.input(Input::Up(Button::Left, Pt { x, y }));
    o
}

fn select(s: &mut Session) {
    s.input(Input::Down(Button::Left, Pt { x: 100, y: 100 }));
    s.input(Input::Move(Pt { x: 300, y: 250 }));
    s.input(Input::Up(Button::Left, Pt { x: 500, y: 400 }));
}

fn session_with_findings() -> Session {
    let mut s = Session::new(800, 600);
    s.start_detect();
    assert_eq!(s.detect(), Detect::Searching);
    s.set_findings(vec![
        Finding { category: Category::Email, rects: vec![r(150, 150, 120, 16)] },
        Finding { category: Category::SeedPhrase, rects: vec![r(150, 200, 200, 16), r(150, 220, 180, 16)] },
        // вне зоны — не показывается и не закрашивается «всё»
        Finding { category: Category::Ip, rects: vec![r(600, 500, 80, 16)] },
    ]);
    select(&mut s);
    s
}

#[test]
fn only_findings_inside_the_zone_count() {
    let s = session_with_findings();
    assert!(s.danger(), "режим «Опасное» включается вместе с поиском");
    assert_eq!(s.findings().count(), 2);
    assert!(s.redactions().is_empty());
}

#[test]
fn click_redacts_and_second_click_restores() {
    let mut s = session_with_findings();
    assert_eq!(click(&mut s, 160, 225), Outcome::Redraw, "щелчок по второй строке сид-фразы");
    assert_eq!(s.redactions(), vec![r(150, 200, 200, 16), r(150, 220, 180, 16)], "вся фраза целиком");
    assert_eq!(s.selection(), Some(r(100, 100, 400, 300)), "зона не сдвинулась");
    click(&mut s, 160, 225);
    assert!(s.redactions().is_empty());
}

#[test]
fn shift_d_redacts_everything_in_zone_and_d_hides_highlights() {
    let mut s = session_with_findings();
    assert_eq!(key(&mut s, 'D', true), Outcome::Redraw);
    assert_eq!(s.redactions().len(), 3, "почта и две строки фразы, но не IP вне зоны");
    key(&mut s, 'D', false);
    assert!(!s.danger());
    // без подсветки щелчок по находке — обычный щелчок (сдвиг зоны), не снимает закраску
    click(&mut s, 160, 155);
    assert_eq!(s.redactions().len(), 3);
}

#[test]
fn hover_shows_category() {
    let mut s = session_with_findings();
    s.input(Input::Move(Pt { x: 160, y: 155 }));
    assert_eq!(s.hover_finding().map(|f| f.category), Some(Category::Email));
    s.input(Input::Move(Pt { x: 450, y: 350 }));
    assert_eq!(s.hover_finding(), None);
}

#[test]
fn drawing_tool_draws_over_findings_redact_tool_toggles() {
    let mut s = session_with_findings();
    key(&mut s, 'A', false);
    s.input(Input::Down(Button::Left, Pt { x: 160, y: 155 }));
    s.input(Input::Move(Pt { x: 300, y: 300 }));
    s.input(Input::Up(Button::Left, Pt { x: 300, y: 300 }));
    assert_eq!(s.marks().items().len(), 1, "со стрелкой щелчок рисует, а не закрашивает");
    assert!(s.redactions().is_empty());
    key(&mut s, 'B', false);
    assert_eq!(s.tool(), Some(Tool::Redact));
    click(&mut s, 160, 155);
    assert_eq!(s.redactions().len(), 1, "с инструментом «Закраска» щелчок по находке закрашивает её");
}

#[test]
fn manual_redaction_is_a_mark_with_undo() {
    let mut s = Session::new(800, 600);
    select(&mut s);
    key(&mut s, 'B', false);
    s.input(Input::Down(Button::Left, Pt { x: 200, y: 200 }));
    s.input(Input::Move(Pt { x: 260, y: 230 }));
    s.input(Input::Up(Button::Left, Pt { x: 300, y: 260 }));
    assert!(matches!(s.marks().items()[0].shape, Shape::Redact(..)));
    s.input(Input::Key { key: Key::Letter('Z'), shift: false, ctrl: true });
    assert!(s.marks().items().is_empty(), "Ctrl+Z снимает закраску");
}

/// SC-005: закрашенное место не зависит от исходных пикселей — одинаковое при любом снимке.
#[test]
fn redacted_pixels_carry_nothing_from_the_source() {
    let noise = |seed: u32| {
        let mut x = seed;
        let px = (0..400 * 300)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                0xFF00_0000 | (x & 0x00FF_FFFF)
            })
            .collect();
        Image::from_pixels(400, 300, px)
    };
    let sel = r(50, 40, 300, 200);
    let manual = zyrik::annot::Mark {
        shape: Shape::Redact(zyrik::annot::P::new(60.5, 50.5), zyrik::annot::P::new(120.2, 90.7)),
        color: Default::default(),
        width: 4.0,
    };
    let found = [r(200, 100, 90, 18), r(200, 120, 70, 18)];
    let a = bake(&noise(1), sel, std::slice::from_ref(&manual), None, &found);
    let b = bake(&noise(2), sel, std::slice::from_ref(&manual), None, &found);
    // ручная закраска округляется наружу: от floor(60.5)=60 до ceil(120.2)=121
    let zones = [r(60, 50, 61, 41), r(200, 100, 90, 18), r(200, 120, 70, 18)];
    for z in zones {
        for y in z.y..z.bottom() {
            for x in z.x..z.right() {
                let (sx, sy) = ((x - sel.x) as u32, (y - sel.y) as u32);
                assert_eq!(a.get(sx, sy), REDACT, "({x},{y}) не закрашен целиком");
                assert_eq!(a.get(sx, sy), b.get(sx, sy));
            }
        }
    }
    // а вне закраски снимки по-прежнему разные
    assert_ne!(a.get(5, 5), b.get(5, 5));
}
