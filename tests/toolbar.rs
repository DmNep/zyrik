//! T029: где стоит панель и куда попадает клик.

use zyrik::annot::{Color, Tool};
use zyrik::platform::Monitor;
use zyrik::session::{Pt, Rect};
use zyrik::toolbar::{layout, Button, GAP};

const SCREEN: Rect = Rect { x: 0, y: 0, w: 1920, h: 1080 };

fn one_monitor() -> Vec<Monitor> {
    vec![Monitor { rect: SCREEN, scale: 100 }]
}

fn r(x: i32, y: i32, w: i32, h: i32) -> Rect {
    Rect { x, y, w, h }
}

fn inside(inner: Rect, outer: Rect) -> bool {
    inner.x >= outer.x && inner.y >= outer.y && inner.right() <= outer.right() && inner.bottom() <= outer.bottom()
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
}

#[test]
fn below_right_outside_by_default() {
    let sel = r(400, 300, 800, 400);
    let bar = layout(sel, SCREEN, &one_monitor());
    assert_eq!(bar.rect.right(), sel.right(), "выровнена по правому краю");
    assert_eq!(bar.rect.y, sel.bottom() + GAP);
    assert!(!overlaps(bar.rect, sel), "не загораживает выделенное");
    assert_eq!(bar.buttons.len(), 11);
}

#[test]
fn bottom_edge_moves_bar_above() {
    let sel = r(400, 700, 800, 370);
    let bar = layout(sel, SCREEN, &one_monitor());
    assert_eq!(bar.rect.bottom(), sel.y - GAP);
    assert!(inside(bar.rect, SCREEN));
    assert!(!overlaps(bar.rect, sel));
}

#[test]
fn full_screen_selection_puts_bar_inside() {
    let bar = layout(SCREEN, SCREEN, &one_monitor());
    assert!(inside(bar.rect, SCREEN));
    assert_eq!(bar.rect.bottom(), SCREEN.bottom() - GAP);
}

#[test]
fn small_selection_at_left_edge_keeps_bar_on_screen() {
    let sel = r(0, 100, 60, 40);
    let bar = layout(sel, SCREEN, &one_monitor());
    assert_eq!(bar.rect.x, 0);
    assert!(inside(bar.rect, SCREEN));
}

#[test]
fn bar_stays_on_the_monitor_of_the_selection() {
    // два монитора: левый 1920×1080 (100%), правый 2560×1440 (150%), выровнены по верху
    let mons = vec![
        Monitor { rect: r(0, 0, 1920, 1080), scale: 100 },
        Monitor { rect: r(1920, 0, 2560, 1440), scale: 150 },
    ];
    let screen = r(0, 0, 4480, 1440);
    // зона у нижнего края левого монитора: там места нет, хотя на правом ниже ещё есть
    let sel = r(1000, 900, 400, 175);
    let bar = layout(sel, screen, &mons);
    assert!(inside(bar.rect, mons[0].rect), "{bar:?}");
    assert_eq!(bar.scale, 100);
    // на правом мониторе кнопки крупнее
    let big = layout(r(2500, 300, 600, 300), screen, &mons);
    assert_eq!(big.scale, 150);
    assert_eq!(big.buttons[0].1.w, 48);
}

#[test]
fn click_hits_buttons() {
    let bar = layout(r(400, 300, 800, 400), SCREEN, &one_monitor());
    for (b, rect) in &bar.buttons {
        let c = Pt { x: rect.x + rect.w / 2, y: rect.y + rect.h / 2 };
        assert_eq!(bar.hit(c), Some(*b));
    }
    let order: Vec<Button> = bar.buttons.iter().map(|(b, _)| *b).collect();
    assert_eq!(order[0], Button::Tool(Tool::Pen));
    assert_eq!(order[5], Button::Color(Color::Red));
    assert_eq!(order[10], Button::Close);
    // промежуток между группами — не кнопка
    let tools_end = bar.buttons[4].1.right();
    assert_eq!(bar.hit(Pt { x: tools_end + 2, y: bar.rect.y + bar.rect.h / 2 }), None);
    assert!(bar.contains(Pt { x: tools_end + 2, y: bar.rect.y + bar.rect.h / 2 }));
}
