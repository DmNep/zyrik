//! Помощник для проверки под GNOME (Mutter): ввод через D-Bus `org.gnome.Mutter.RemoteDesktop`.
//! Не часть Зырика — только для tools/wayland-desktops. Сеанс живёт, пока жив этот процесс.
//! Команды построчно со stdin: drag X1 Y1 X2 Y2 · click X Y [right] · key КОД… (evdev; ctrl+31)

#[cfg(target_os = "linux")]
fn main() {
    use std::time::Duration;
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::OwnedObjectPath;

    let conn = Connection::session().expect("D-Bus");
    let rd = Proxy::new(&conn, "org.gnome.Mutter.RemoteDesktop", "/org/gnome/Mutter/RemoteDesktop", "org.gnome.Mutter.RemoteDesktop").unwrap();
    let path: OwnedObjectPath = rd.call("CreateSession", &()).expect("CreateSession");
    let s = Proxy::new(&conn, "org.gnome.Mutter.RemoteDesktop", path.as_str(), "org.gnome.Mutter.RemoteDesktop.Session").unwrap();
    let _: () = s.call("Start", &()).expect("Start");
    let pause = || std::thread::sleep(Duration::from_millis(30));
    // без потока ScreenCast абсолютных координат нет: упираем курсор в левый верхний угол и
    // двигаем относительно
    let mut at = (0.0f64, 0.0f64);
    let home = |at: &mut (f64, f64)| {
        for _ in 0..4 {
            let _: () = s.call("NotifyPointerMotionRelative", &(-5000.0f64, -5000.0f64)).unwrap();
            pause();
        }
        *at = (0.0, 0.0);
    };
    let to = |at: &mut (f64, f64), x: f64, y: f64| {
        let _: () = s.call("NotifyPointerMotionRelative", &(x - at.0, y - at.1)).unwrap();
        *at = (x, y);
        pause();
    };
    let btn = |b: i32, down: bool| {
        let _: () = s.call("NotifyPointerButton", &(b, down)).unwrap();
        pause();
    };
    let key = |k: u32, down: bool| {
        let _: () = s.call("NotifyKeyboardKeycode", &(k, down)).unwrap();
        pause();
    };
    home(&mut at);
    for line in std::io::stdin().lines().map_while(Result::ok) {
        let c: Vec<&str> = line.split_whitespace().collect();
        let n = |i: usize| c.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        match c.first().copied() {
            Some("click") => {
                let b = if c.get(3) == Some(&"right") { 0x111 } else { 0x110 };
                to(&mut at, n(1), n(2));
                btn(b, true);
                btn(b, false);
            }
            Some("drag") => {
                let (x1, y1, x2, y2) = (n(1), n(2), n(3), n(4));
                to(&mut at, x1, y1);
                btn(0x110, true);
                for k in 1..=5 {
                    let t = f64::from(k) / 5.0;
                    to(&mut at, x1 + (x2 - x1) * t, y1 + (y2 - y1) * t);
                }
                btn(0x110, false);
            }
            Some("key") => {
                for k in &c[1..] {
                    let (ctrl, code) = match k.strip_prefix("ctrl+") {
                        Some(r) => (true, r),
                        None => (false, *k),
                    };
                    let code: u32 = code.parse().unwrap_or(0);
                    if ctrl {
                        key(29, true);
                    }
                    key(code, true);
                    key(code, false);
                    if ctrl {
                        key(29, false);
                    }
                }
            }
            _ => eprintln!("mutter_input: drag|click|key"),
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
