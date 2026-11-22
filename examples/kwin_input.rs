//! Помощник для проверки под KDE Plasma (KWin): ввод через `org_kde_kwin_fake_input`.
//! Не часть Зырика — только для tools/wayland-desktops. Команды построчно со stdin:
//!   move X Y · drag X1 Y1 X2 Y2 · click X Y [right] · key КОД… (evdev; «ctrl+31» — с Ctrl)

#[cfg(target_os = "linux")]
fn main() {
    use std::time::Duration;
    use wayland_client::globals::{registry_queue_init, GlobalListContents};
    use wayland_client::protocol::wl_registry::WlRegistry;
    use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle};
    use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;

    struct S;
    impl Dispatch<WlRegistry, GlobalListContents> for S {
        fn event(_: &mut S, _: &WlRegistry, _: <WlRegistry as Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<S>) {}
    }
    delegate_noop!(S: ignore OrgKdeKwinFakeInput);

    let conn = Connection::connect_to_env().expect("Wayland");
    let (globals, mut q) = registry_queue_init::<S>(&conn).expect("registry");
    let qh = q.handle();
    let fi: OrgKdeKwinFakeInput = globals.bind(&qh, 4..=5, ()).expect("org_kde_kwin_fake_input");
    fi.authenticate("zyrik-test".into(), "проверка Зырика".into());
    q.roundtrip(&mut S).unwrap();
    let mut sync = || {
        q.roundtrip(&mut S).unwrap();
        std::thread::sleep(Duration::from_millis(30));
    };
    for line in std::io::stdin().lines().map_while(Result::ok) {
        let c: Vec<&str> = line.split_whitespace().collect();
        let n = |i: usize| c.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        match c.first().copied() {
            Some("move") => {
                fi.pointer_motion_absolute(n(1), n(2));
                sync();
            }
            Some("click") => {
                let b = if c.get(3) == Some(&"right") { 0x111 } else { 0x110 };
                fi.pointer_motion_absolute(n(1), n(2));
                sync();
                fi.button(b, 1);
                sync();
                fi.button(b, 0);
                sync();
            }
            Some("drag") => {
                let (x1, y1, x2, y2) = (n(1), n(2), n(3), n(4));
                fi.pointer_motion_absolute(x1, y1);
                sync();
                fi.button(0x110, 1);
                sync();
                for k in 1..=5 {
                    let t = f64::from(k) / 5.0;
                    fi.pointer_motion_absolute(x1 + (x2 - x1) * t, y1 + (y2 - y1) * t);
                    sync();
                }
                fi.button(0x110, 0);
                sync();
            }
            Some("key") => {
                for k in &c[1..] {
                    let (ctrl, code) = match k.strip_prefix("ctrl+") {
                        Some(rest) => (true, rest),
                        None => (false, *k),
                    };
                    let code: u32 = code.parse().unwrap_or(0);
                    if ctrl {
                        fi.keyboard_key(29, 1);
                        sync();
                    }
                    fi.keyboard_key(code, 1);
                    sync();
                    fi.keyboard_key(code, 0);
                    sync();
                    if ctrl {
                        fi.keyboard_key(29, 0);
                        sync();
                    }
                }
            }
            _ => eprintln!("kwin_input: move|drag|click|key"),
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {}
