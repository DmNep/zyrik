//! T025: вырезание зоны пиксель в пиксель, в том числе через границу мониторов (FR-012).
//! T027: затирание буферов.

use zyrik::image::Image;
use zyrik::session::Rect;

/// Виртуальный экран из двух мониторов разного масштаба, как его отдаёт захват под
/// Per-Monitor DPI v2: всё уже в физических пикселях. Слева «4K 150%» (тут 300×200),
/// справа «FullHD 100%» (200×120, выровнен по верху), снизу справа — пустота вне мониторов.
fn two_monitors() -> Image {
    let (w, h) = (500u32, 200u32);
    let mut px = vec![0u32; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let on_left = x < 300;
            let on_right = x >= 300 && y < 120;
            // у каждого пикселя своё значение: так видно любой сдвиг на 1 px
            px[(y * w + x) as usize] = if on_left || on_right { 0xFF00_0000 | (y << 12) | x } else { 0 };
        }
    }
    Image::from_pixels(w, h, px)
}

#[test]
fn crop_across_monitor_border_is_pixel_exact() {
    let img = two_monitors();
    let r = Rect { x: 250, y: 50, w: 120, h: 40 };
    let c = img.crop(r);
    assert_eq!((c.width(), c.height()), (120, 40));
    for y in 0..40 {
        for x in 0..120 {
            assert_eq!(c.get(x, y), img.get(250 + x, 50 + y), "{x},{y}");
        }
    }
    // шов мониторов попал ровно в столбец 50
    assert_eq!(c.get(49, 0) & 0xFFF, 299);
    assert_eq!(c.get(50, 0) & 0xFFF, 300);
}

#[test]
fn crop_keeps_area_outside_monitors_as_is() {
    let img = two_monitors();
    let c = img.crop(Rect { x: 280, y: 100, w: 40, h: 40 });
    assert_eq!(c.get(10, 10), img.get(290, 110));
    assert_eq!(c.get(30, 30), 0, "вне мониторов");
}

#[test]
fn crop_is_cut_to_image() {
    let img = two_monitors();
    let c = img.crop(Rect { x: 480, y: 190, w: 100, h: 100 });
    assert_eq!((c.width(), c.height()), (20, 10));
}

#[test]
fn wipe_zeroes_every_pixel_and_keeps_size() {
    let mut img = two_monitors();
    assert!(img.pixels().iter().any(|p| *p != 0));
    img.wipe();
    assert_eq!((img.width(), img.height()), (500, 200));
    assert_eq!(img.pixels().len(), 500 * 200);
    assert!(img.pixels().iter().all(|p| *p == 0));
}

#[test]
fn debug_never_prints_pixels() {
    let img = two_monitors();
    assert_eq!(format!("{img:?}"), "Image(500×200)");
}
