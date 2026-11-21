//! T031: модель пометок, привязка Shift, undo/redo, толщина.

use zyrik::annot::*;

fn line(x: f32) -> Mark {
    Mark { shape: Shape::Line(P::new(0.0, 0.0), P::new(x, 0.0)), color: Color::Red, width: 4.0 }
}

#[test]
fn undo_redo() {
    let mut m = Marks::new();
    assert!(!m.undo());
    m.push(line(10.0));
    m.push(line(20.0));
    assert!(m.undo());
    assert_eq!(m.items(), [line(10.0)]);
    assert!(m.redo());
    assert_eq!(m.items(), [line(10.0), line(20.0)]);
    assert!(!m.redo());
}

#[test]
fn new_mark_drops_redo() {
    let mut m = Marks::new();
    m.push(line(10.0));
    m.undo();
    m.push(line(30.0));
    assert!(!m.redo(), "после новой пометки отменённое не возвращается");
    assert_eq!(m.items(), [line(30.0)]);
}

#[test]
fn click_and_empty_text_are_not_marks() {
    let mut m = Marks::new();
    m.push(line(1.0));
    m.push(Mark { shape: Shape::Text { at: P::new(5.0, 5.0), text: "  ".into(), size: 24.0 }, color: Color::Blue, width: 4.0 });
    assert!(m.is_empty());
    m.push(Mark { shape: Shape::Pen(vec![P::new(3.0, 3.0)]), color: Color::Red, width: 4.0 });
    assert_eq!(m.items().len(), 1, "точка карандашом — это пометка");
}

fn close(a: P, b: P) -> bool {
    (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01
}

#[test]
fn shift_snaps_line_to_45_degrees() {
    let a = P::new(100.0, 100.0);
    // почти горизонтально → горизонталь той же длины
    let b = snap45(a, P::new(200.0, 108.0));
    assert!((b.y - 100.0).abs() < 0.01 && b.x > 199.0, "{b:?}");
    // почти вертикально вверх
    let b = snap45(a, P::new(96.0, 20.0));
    assert!((b.x - 100.0).abs() < 0.01 && b.y < 21.0, "{b:?}");
    // около 45° вниз-влево
    let b = snap45(a, P::new(40.0, 155.0));
    assert!(((b.x - a.x).abs() - (b.y - a.y).abs()).abs() < 0.01 && b.x < a.x && b.y > a.y, "{b:?}");
    assert!(close(snap45(a, a), a));
}

#[test]
fn shift_makes_square_towards_cursor() {
    let a = P::new(10.0, 10.0);
    assert!(close(square(a, P::new(60.0, 30.0)), P::new(60.0, 60.0)));
    assert!(close(square(a, P::new(-20.0, 0.0)), P::new(-20.0, -20.0)));
}

#[test]
fn width_and_text_size_limits() {
    assert_eq!(clamp_width(0.0), 1.0);
    assert_eq!(clamp_width(100.0), 40.0);
    assert_eq!(clamp_text(5.0), MIN_TEXT);
    assert_eq!(clamp_text(500.0), MAX_TEXT);
}

#[test]
fn tool_keys() {
    for (c, t) in [('p', Tool::Pen), ('L', Tool::Line), ('a', Tool::Arrow), ('R', Tool::Rect), ('t', Tool::Text)] {
        assert_eq!(Tool::from_key(c), Some(t));
    }
    assert_eq!(Tool::from_key('C'), None, "C — цвет, не инструмент");
    assert_eq!(Color::Red.other(), Color::Blue);
}
