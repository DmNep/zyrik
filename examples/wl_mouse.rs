//! Помощник для проверки под Wayland (Sway и др. wlroots): виртуальная мышь.
//! Не часть Зырика — только для tools/wayland-test.
//!
//!   wl_mouse W H move X Y        — курсор в точку (W×H — размер экрана)
//!   wl_mouse W H drag X1 Y1 X2 Y2 — протянуть левой кнопкой
//!   wl_mouse W H click X Y [right]
//!   wl_mouse W H -                — команды построчно со stdin, одна мышь на всё время

#[cfg(target_os = "linux")]
fn main() {
    use std::time::Duration;
    use wayland_client::globals::{registry_queue_init, GlobalListContents};
    use wayland_client::protocol::{wl_pointer::ButtonState, wl_registry::WlRegistry, wl_seat::WlSeat};
    use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle};
    use wayland_protocols_wlr::virtual_pointer::v1::client::{
        zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
    };

    struct S;
    impl Dispatch<WlRegistry, GlobalListContents> for S {
        fn event(_: &mut S, _: &WlRegistry, _: <WlRegistry as Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<S>) {}
    }
    delegate_noop!(S: ignore WlSeat);
    delegate_noop!(S: ignore ZwlrVirtualPointerManagerV1);
    delegate_noop!(S: ignore ZwlrVirtualPointerV1);

    let a: Vec<String> = std::env::args().skip(1).collect();
    let n = |i: usize| a.get(i).and_then(|v| v.parse::<u32>().ok()).expect("число");
    let (w, h) = (n(0), n(1));
    let conn = Connection::connect_to_env().expect("Wayland");
    let (globals, mut q) = registry_queue_init::<S>(&conn).expect("registry");
    let qh = q.handle();
    let seat: WlSeat = globals.bind(&qh, 1..=7, ()).expect("seat");
    let mgr: ZwlrVirtualPointerManagerV1 = globals.bind(&qh, 1..=2, ()).expect("virtual pointer");
    let p = mgr.create_virtual_pointer(Some(&seat), &qh, ());
    // одно действие мыши → кадр → дождаться, что композитор его принял
    enum A {
        To(u32, u32),
        Btn(u32, ButtonState),
    }
    let mut t = 0u32;
    let mut run = |acts: &[A]| {
        for a in acts {
            t += 16;
            match *a {
                A::To(x, y) => p.motion_absolute(t, x, y, w, h),
                A::Btn(b, s) => p.button(t, b, s),
            }
            p.frame();
            q.roundtrip(&mut S).unwrap();
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    // одна команда — из аргументов; `-` — команды построчно со стандартного ввода
    // (одна мышь на всё время, как у настоящего компьютера)
    let mut exec = |cmd: &[&str]| {
        let n = |i: usize| cmd.get(i).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
        match cmd.first().copied() {
            Some("move") => run(&[A::To(n(1) as u32, n(2) as u32)]),
            Some("click") => {
                let b = if cmd.get(3) == Some(&"right") { 0x111 } else { 0x110 };
                run(&[A::To(n(1) as u32, n(2) as u32), A::Btn(b, ButtonState::Pressed), A::Btn(b, ButtonState::Released)]);
            }
            Some("drag") => {
                let (x1, y1, x2, y2) = (n(1), n(2), n(3), n(4));
                let mut acts = vec![A::To(x1 as u32, y1 as u32), A::Btn(0x110, ButtonState::Pressed)];
                for k in 1..=5 {
                    acts.push(A::To((x1 + (x2 - x1) * k / 5) as u32, (y1 + (y2 - y1) * k / 5) as u32));
                }
                acts.push(A::Btn(0x110, ButtonState::Released));
                run(&acts);
            }
            _ => eprintln!("wl_mouse W H move|click|drag … | -"),
        }
    };
    if a.get(2).map(String::as_str) == Some("-") {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            let cmd: Vec<&str> = line.split_whitespace().collect();
            exec(&cmd);
        }
    } else {
        let cmd: Vec<&str> = a[2..].iter().map(String::as_str).collect();
        exec(&cmd);
    }
    p.destroy();
    q.roundtrip(&mut S).unwrap();
}

#[cfg(not(target_os = "linux"))]
fn main() {}
