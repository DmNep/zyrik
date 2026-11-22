//! Встроенное распознавание + поиск опасного на нарисованном «скриншоте» (фича `ocr`).
//!   cargo test --features ocr --test ocr

#![cfg(feature = "ocr")]

use std::collections::HashSet;

use zyrik::annot::text;
use zyrik::image::Image;
use zyrik::redact::{detect_all, Category};
use zyrik::session::Rect;

fn screenshot(lines: &[&str], size: f32) -> Image {
    let (w, lh) = (1400u32, (size * 1.6) as u32);
    let h = lh * lines.len() as u32 + 40;
    let mut img = Image::from_pixels(w, h, vec![0xFFF5_F5F5; (w * h) as usize]);
    let clip = Rect { x: 0, y: 0, w: w as i32, h: h as i32 };
    for (i, l) in lines.iter().enumerate() {
        text::draw(&mut img, 20, 20 + (i as u32 * lh) as i32, l, size, 0xFF20_2020, clip);
    }
    img
}

#[test]
fn finds_secrets_on_a_rendered_screenshot() {
    assert!(zyrik::ocr::available());
    let img = screenshot(
        &[
            "Пишите на ivan.petrov@example.com или звоните +7 (912) 345-67-89",
            "Карта 4111 1111 1111 1111 до 12/27, СНИЛС 112-233-445 95",
            "export DB_PASSWORD=hunter2secret",
            "git clone https://deploy:Zk8qL2mX9vB4nR7t@git.corp/app.git",
            "ssh root@10.20.30.40 -i C:\\Users\\ivanov\\.ssh\\id_rsa",
            "1. abandon 2. ability 3. able 4. about 5. above 6. absent",
            "7. absorb 8. abstract 9. absurd 10. abuse 11. access 12. accident",
            "send 0.5 ETH to 0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            "Исполнитель: Петров Иван Сергеевич, пароль: Qwerty123",
        ],
        15.0,
    );
    let t0 = std::time::Instant::now();
    let sets = zyrik::ocr::recognize(&img);
    let found = detect_all(&sets);
    let cats: HashSet<Category> = found.iter().map(|f| f.category).collect();
    println!("распознано за {:?}: {cats:?}", t0.elapsed());
    for c in [
        Category::Email,
        Category::Phone,
        Category::Card,
        Category::Snils,
        Category::Password,
        Category::UrlCredentials,
        Category::Ip,
        Category::UserPath,
        Category::SeedPhrase,
        Category::Wallet,
        Category::Name,
    ] {
        assert!(cats.contains(&c), "не найдено: {c:?}; всё найденное: {cats:?}");
    }
}

#[test]
fn ordinary_code_stays_clean() {
    let img = screenshot(
        &[
            "use std::sync::Arc;",
            "fn main() {",
            "    let v: Vec<u32> = Vec::new();",
            "    println!(\"{}\", v.len());",
            "}",
            "Версия 1.2.3 вышла 21.11.2026 в 14:05:33.",
        ],
        15.0,
    );
    let found = detect_all(&zyrik::ocr::recognize(&img));
    assert!(found.len() <= 1, "SC-008: не больше одного ложного срабатывания: {found:?}");
}
