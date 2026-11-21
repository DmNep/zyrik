//! T033: эталонные картинки. Рисуем оверлей поверх искусственного кадра — все инструменты,
//! подпись, панель с подсказкой — и сравниваем с `tests/golden/*.png`.
//!
//! Обновить эталоны (после осознанной правки внешнего вида):
//!   ZYRIK_BLESS=1 cargo test --test golden
//! и посмотреть на картинки глазами перед коммитом.

use std::path::PathBuf;

use zyrik::annot::render;
use zyrik::export::encode_png;
use zyrik::i18n::Lang;
use zyrik::image::Image;
use zyrik::session::{Button, Input, Key, Pt, Session};

const W: u32 = 900;
const H: u32 = 600;

/// Кадр-заглушка: мягкий градиент и сетка — видно и затемнение, и сдвиги.
fn frame() -> Image {
    let mut px = Vec::with_capacity((W * H) as usize);
    for y in 0..H {
        for x in 0..W {
            let grid = x % 50 == 0 || y % 50 == 0;
            let (r, g, b) = if grid { (200, 200, 210) } else { (230 - y * 60 / H, 236, 220 + x * 30 / W) };
            px.push(0xFF00_0000 | (r << 16) | (g << 8) | b);
        }
    }
    Image::from_pixels(W, H, px)
}

fn down(s: &mut Session, x: i32, y: i32) {
    s.input(Input::Down(Button::Left, Pt { x, y }));
}
fn mv(s: &mut Session, x: i32, y: i32) {
    s.input(Input::Move(Pt { x, y }));
}
fn up(s: &mut Session, x: i32, y: i32) {
    s.input(Input::Up(Button::Left, Pt { x, y }));
}
fn drag(s: &mut Session, a: (i32, i32), b: (i32, i32)) {
    down(s, a.0, a.1);
    mv(s, (a.0 + b.0) / 2, (a.1 + b.1) / 2);
    mv(s, b.0, b.1);
    up(s, b.0, b.1);
}
fn key(s: &mut Session, c: char) {
    s.input(Input::Key { key: Key::Letter(c), shift: false, ctrl: false });
}

fn scene() -> Session {
    let mut s = Session::new(W, H);
    drag(&mut s, (120, 80), (720, 440));
    key(&mut s, 'R');
    drag(&mut s, (160, 120), (360, 220));
    key(&mut s, 'A');
    s.input(Input::Wheel(2, Pt { x: 0, y: 0 }));
    drag(&mut s, (520, 330), (390, 230));
    key(&mut s, 'C'); // синий
    key(&mut s, 'L');
    drag(&mut s, (160, 300), (420, 300));
    key(&mut s, 'P');
    down(&mut s, 470, 140);
    for i in 0..40 {
        let t = i as f32 / 39.0;
        mv(&mut s, 470 + (t * 200.0) as i32, 140 + ((t * 12.0).sin() * 25.0) as i32);
    }
    up(&mut s, 670, 140);
    key(&mut s, 'C'); // обратно красный
    key(&mut s, 'T');
    down(&mut s, 170, 350);
    for c in "Тут баг → Zyrik".chars() {
        s.input(Input::Char(c));
    }
    s.input(Input::Key { key: Key::Enter, shift: false, ctrl: false });
    key(&mut s, 'A');
    // курсор над кнопкой «Стрелка» — видна подсказка
    let b = s.toolbar().unwrap().button_rect(zyrik::toolbar::Button::Tool(zyrik::annot::Tool::Arrow)).unwrap();
    mv(&mut s, b.x + 5, b.y + 5);
    s
}

fn check(name: &str, img: &Image) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name);
    let png = encode_png(img);
    if std::env::var_os("ZYRIK_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &*png).unwrap();
        return;
    }
    let want = std::fs::read(&path).unwrap_or_else(|_| panic!("нет эталона {}, запустите с ZYRIK_BLESS=1", path.display()));
    assert!(*png == want, "{name} отличается от эталона; посмотрите и обновите с ZYRIK_BLESS=1");
}

#[test]
fn overlay_with_all_tools() {
    let f = frame();
    let s = scene();
    let mut out = Image::new(W, H);
    render::overlay(&f, &s, &mut out, Lang::from_code("ru").unwrap());
    check("overlay_ru.png", &out);
}

#[test]
fn baked_shot_has_marks_but_no_ui() {
    let f = frame();
    let s = scene();
    let sel = s.selection().unwrap();
    let shot = render::bake(&f, sel, s.marks().items(), None);
    check("baked.png", &shot);
}
