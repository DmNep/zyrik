//! Прослойка Linux X11 (фаза 4). Ядро то же, что в Windows; здесь только X-протокол.
//!
//! Потоки: главный — ядро (`app::run`) и запросы к X-серверу; поток событий читает события X
//! и превращает их в команды ядра, а запросы буфера обмена обслуживает сам (их надо отвечать
//! сразу, даже пока ядро занято). Соединение `RustConnection` можно делить между потоками.
//!
//! Принцип I: соединения только через unix-сокеты (X-сервер, D-Bus), на диск ничего не пишем.

mod clipboard;
mod conn;
mod keys;
mod tray;

use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConfigureWindowAux, ConnectionExt, CreateGCAux,
    CreateWindowAux, EventMask, PropMode, GrabMode, GrabStatus, ImageFormat, ImageOrder, InputFocus, KeyButMask, Rectangle,
    StackMode, WindowClass,
};
use x11rb::protocol::{ErrorKind, Event as XEvent};
use x11rb::rust_connection::{ReplyError, RustConnection};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

use zyrik::export::Stamp;
use zyrik::hotkey::Hotkey;
use zyrik::image::Image;
use zyrik::platform::{Command, Frame, MenuItem, Monitor, Place, PlaceKind, Platform, Problem, Shot};
use zyrik::session::{Button, Cursor, Input, Key, Pt, Rect};

use keys::Keymap;

/// Что поток событий передаёт главному.
pub enum Event {
    Cmd(Command),
    /// Соединение с X-сервером потеряно — выходим
    Quit,
}

struct Atoms {
    clipboard: u32,
    targets: u32,
    image_png: u32,
    kde_hint: u32,
    incr: u32,
    net_active_window: u32,
}

/// Общее для главного потока и потока событий.
pub struct X {
    conn: RustConnection,
    root: u32,
    /// Окно выделения: без рамки, поверх всего (override-redirect)
    overlay: u32,
    /// Невидимое окно — владелец CLIPBOARD
    owner: u32,
    gc: u32,
    depth: u8,
    atoms: Atoms,
    keymap: Mutex<Keymap>,
    clip: Mutex<clipboard::Clip>,
    /// (код клавиши, модификаторы, что снимать)
    hotkeys: Mutex<Vec<(u8, u16, Shot)>>,
    shown: AtomicBool,
}

pub struct Linux {
    x: Arc<X>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    timers: Vec<(Instant, Command)>,
    tray: Option<ksni::blocking::Handle<tray::ZTray>>,
    guard: Option<UnixListener>,
    cursors: [u32; 8],
    cursor: Option<Cursor>,
    size: (u16, u16),
}

fn os<E: std::fmt::Display>(e: E) -> Problem {
    Problem::Os(e.to_string())
}

fn atom(conn: &RustConnection, name: &str) -> Result<u32, String> {
    Ok(conn.intern_atom(false, name.as_bytes()).map_err(|e| e.to_string())?.reply().map_err(|e| e.to_string())?.atom)
}

/// Стандартные курсоры X (шрифт `cursor`): номера глифов из `X11/cursorfont.h`.
fn glyph(c: Cursor) -> u16 {
    match c {
        Cursor::Cross => 34,       // crosshair
        Cursor::Arrow => 68,       // left_ptr
        Cursor::Text => 152,       // xterm
        Cursor::Move => 52,        // fleur
        Cursor::ResizeNwse => 14,  // bottom_right_corner
        Cursor::ResizeNesw => 12,  // bottom_left_corner
        Cursor::ResizeNs => 116,   // sb_v_double_arrow
        Cursor::ResizeWe => 108,   // sb_h_double_arrow
    }
}

const CURSORS: [Cursor; 8] = [
    Cursor::Cross,
    Cursor::Arrow,
    Cursor::Text,
    Cursor::Move,
    Cursor::ResizeNwse,
    Cursor::ResizeNesw,
    Cursor::ResizeNs,
    Cursor::ResizeWe,
];

impl Linux {
    pub fn new() -> Result<Linux, String> {
        let (conn, screen) = conn::connect()?;
        let s = conn.setup().roots.get(screen).ok_or("нет такого экрана")?.clone();
        let e = |e: x11rb::errors::ConnectionError| e.to_string();
        let ids = || conn.generate_id().map_err(|e| e.to_string());

        let overlay = ids()?;
        let events = EventMask::KEY_PRESS
            | EventMask::KEY_RELEASE
            | EventMask::BUTTON_PRESS
            | EventMask::BUTTON_RELEASE
            | EventMask::POINTER_MOTION
            | EventMask::EXPOSURE;
        let aux = CreateWindowAux::new().override_redirect(1).event_mask(events).background_pixel(s.black_pixel);
        conn.create_window(COPY_DEPTH_FROM_PARENT, overlay, s.root, 0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, 0, &aux)
            .map_err(e)?;
        // имя окна — чтобы его находили xdotool и оконные инструменты; в заголовке его не видно
        conn.change_property8(PropMode::REPLACE, overlay, AtomEnum::WM_NAME, AtomEnum::STRING, b"Zyrik").map_err(e)?;
        let owner = ids()?;
        conn.create_window(0, owner, s.root, -10, -10, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &CreateWindowAux::new())
            .map_err(e)?;
        let gc = ids()?;
        conn.create_gc(gc, overlay, &CreateGCAux::new().foreground(s.black_pixel).graphics_exposures(0))
            .map_err(e)?;

        let font = ids()?;
        conn.open_font(font, b"cursor").map_err(e)?;
        let mut cursors = [0u32; 8];
        for (slot, c) in cursors.iter_mut().zip(CURSORS) {
            *slot = ids()?;
            let g = glyph(c);
            conn.create_glyph_cursor(*slot, font, font, g, g + 1, 0, 0, 0, 0xFFFF, 0xFFFF, 0xFFFF).map_err(e)?;
        }
        let _ = conn.close_font(font);

        let atoms = Atoms {
            clipboard: atom(&conn, "CLIPBOARD")?,
            targets: atom(&conn, "TARGETS")?,
            image_png: atom(&conn, "image/png")?,
            kde_hint: atom(&conn, "x-kde-passwordManagerHint")?,
            incr: atom(&conn, "INCR")?,
            net_active_window: atom(&conn, "_NET_ACTIVE_WINDOW")?,
        };
        let keymap = Keymap::load(&conn)?;
        conn.flush().map_err(e)?;

        let x = Arc::new(X {
            root: s.root,
            overlay,
            owner,
            gc,
            depth: s.root_depth,
            atoms,
            keymap: Mutex::new(keymap),
            clip: Mutex::new(clipboard::Clip::default()),
            hotkeys: Mutex::new(Vec::new()),
            shown: AtomicBool::new(false),
            conn,
        });
        let (tx, rx) = channel();
        {
            let (x, tx) = (Arc::clone(&x), tx.clone());
            std::thread::spawn(move || pump(&x, &tx));
        }
        Ok(Linux {
            x,
            tx,
            rx,
            timers: Vec::new(),
            tray: None,
            guard: None,
            cursors,
            cursor: None,
            size: (s.width_in_pixels, s.height_in_pixels),
        })
    }
}

fn send(tx: &Sender<Event>, i: Input) {
    let _ = tx.send(Event::Cmd(Command::Overlay(i)));
}

/// Поток событий X.
fn pump(x: &Arc<X>, tx: &Sender<Event>) {
    let mut last_click: Option<(Instant, i16, i16)> = None;
    loop {
        let ev = match x.conn.wait_for_event() {
            Ok(ev) => ev,
            Err(_) => {
                let _ = tx.send(Event::Quit);
                return;
            }
        };
        let pt = |ex: i16, ey: i16| Pt { x: i32::from(ex), y: i32::from(ey) };
        match ev {
            XEvent::KeyPress(e) if e.event == x.root => {
                let mods = keys::significant(e.state);
                let shot = x.hotkeys.lock().unwrap().iter().find(|h| h.0 == e.detail && h.1 == mods).map(|h| h.2);
                if let Some(shot) = shot {
                    let _ = tx.send(Event::Cmd(Command::Capture(shot)));
                }
            }
            XEvent::KeyPress(e) if e.event == x.overlay => {
                let km = x.keymap.lock().unwrap();
                if km.is_shift(e.detail) {
                    send(tx, Input::Shift(true));
                    continue;
                }
                let bits = u16::from(e.state);
                let shift = bits & u16::from(KeyButMask::SHIFT) != 0;
                let ctrl = bits & u16::from(KeyButMask::CONTROL) != 0;
                let key = km.key(e.detail);
                if let Some(k) = key {
                    send(tx, Input::Key { key: k, shift, ctrl });
                }
                // как WM_CHAR в Windows: Enter, Backspace, Esc и сочетания с Ctrl — не текст
                let control = matches!(key, Some(Key::Enter | Key::Backspace | Key::Esc));
                if !ctrl && !control {
                    if let Some(c) = km.char(e.detail, e.state) {
                        send(tx, Input::Char(c));
                    }
                }
            }
            XEvent::KeyRelease(e) if e.event == x.overlay => {
                if x.keymap.lock().unwrap().is_shift(e.detail) {
                    send(tx, Input::Shift(false));
                }
            }
            XEvent::ButtonPress(e) if e.event == x.overlay => {
                let p = pt(e.event_x, e.event_y);
                match e.detail {
                    1 => {
                        // двойной щелчок: X его не различает — как в Windows, второй щелчок
                        // в пределах 400 мс и 4 пикселей
                        let now = Instant::now();
                        let double = last_click.is_some_and(|(t, lx, ly)| {
                            now - t < Duration::from_millis(400)
                                && (lx - e.event_x).abs() <= 4
                                && (ly - e.event_y).abs() <= 4
                        });
                        last_click = if double { None } else { Some((now, e.event_x, e.event_y)) };
                        send(tx, if double { Input::DoubleClick(p) } else { Input::Down(Button::Left, p) });
                    }
                    3 => send(tx, Input::Down(Button::Right, p)),
                    4 => send(tx, Input::Wheel(1, p)),
                    5 => send(tx, Input::Wheel(-1, p)),
                    _ => {}
                }
            }
            XEvent::ButtonRelease(e) if e.event == x.overlay => {
                let p = pt(e.event_x, e.event_y);
                match e.detail {
                    1 => send(tx, Input::Up(Button::Left, p)),
                    3 => send(tx, Input::Up(Button::Right, p)),
                    _ => {}
                }
            }
            XEvent::MotionNotify(e) if e.event == x.overlay => send(tx, Input::Move(pt(e.event_x, e.event_y))),
            XEvent::Expose(e) if e.window == x.overlay && e.count == 0 => {
                if x.shown.load(Ordering::Relaxed) {
                    send(tx, Input::Repaint);
                }
            }
            XEvent::SelectionRequest(e) => clipboard::request(x, e),
            XEvent::SelectionClear(e) if e.owner == x.owner => clipboard::lost(x),
            XEvent::PropertyNotify(e) => clipboard::property(x, &e),
            XEvent::MappingNotify(_) => {
                if let Ok(km) = Keymap::load(&x.conn) {
                    *x.keymap.lock().unwrap() = km;
                }
            }
            _ => {}
        }
    }
}

/// Масштаб интерфейса из `Xft.dpi` (так его задают KDE, GNOME, XFCE): 96 → 100 %, 144 → 150 %.
fn scale(x: &X) -> u32 {
    let r = x
        .conn
        .get_property(false, x.root, AtomEnum::RESOURCE_MANAGER, AtomEnum::STRING, 0, 1 << 16)
        .ok()
        .and_then(|c| c.reply().ok());
    let Some(r) = r else { return 100 };
    let text = String::from_utf8_lossy(&r.value);
    text.lines()
        .find_map(|l| l.strip_prefix("Xft.dpi:"))
        .and_then(|v| v.trim().parse::<f32>().ok())
        .map_or(100, |dpi| ((dpi * 100.0 / 96.0).round() as u32).clamp(100, 400))
}

fn monitors(x: &X, w: u16, h: u16) -> Vec<Monitor> {
    let scale = scale(x);
    let list = x.conn.randr_get_monitors(x.root, true).ok().and_then(|c| c.reply().ok());
    let mut out: Vec<Monitor> = list
        .map(|r| {
            r.monitors
                .iter()
                .map(|m| Monitor {
                    rect: Rect { x: i32::from(m.x), y: i32::from(m.y), w: i32::from(m.width), h: i32::from(m.height) },
                    scale,
                })
                .collect()
        })
        .unwrap_or_default();
    if out.is_empty() {
        out.push(Monitor { rect: Rect { x: 0, y: 0, w: i32::from(w), h: i32::from(h) }, scale });
    }
    out
}

/// XDG-папки пользователя (`~/.config/user-dirs.dirs`), домашняя, корень и подключённые диски.
fn places() -> Vec<Place> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut out = Vec::new();
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|h| h.join(".config")));
    let dirs = config.and_then(|c| std::fs::read_to_string(c.join("user-dirs.dirs")).ok()).unwrap_or_default();
    let lookup = |key: &str, fallback: &str| -> Option<PathBuf> {
        let home = home.as_ref()?;
        let value = dirs.lines().find_map(|l| l.trim().strip_prefix(key)?.strip_prefix('=')).map(|v| v.trim().trim_matches('"'));
        let path = match value {
            Some(v) => PathBuf::from(v.replace("$HOME", &home.to_string_lossy())),
            None => home.join(fallback),
        };
        (path != *home && path.is_dir()).then_some(path)
    };
    for (kind, key, fallback) in [
        (PlaceKind::Pictures, "XDG_PICTURES_DIR", "Pictures"),
        (PlaceKind::Desktop, "XDG_DESKTOP_DIR", "Desktop"),
        (PlaceKind::Documents, "XDG_DOCUMENTS_DIR", "Documents"),
        (PlaceKind::Downloads, "XDG_DOWNLOAD_DIR", "Downloads"),
    ] {
        if let Some(path) = lookup(key, fallback) {
            out.push(Place { kind, path });
        }
    }
    if let Some(h) = &home {
        out.push(Place { kind: PlaceKind::Home, path: h.clone() });
    }
    out.push(Place { kind: PlaceKind::Drive, path: PathBuf::from("/") });
    // флешки и диски, подключённые через udisks
    let user = std::env::var("USER").unwrap_or_default();
    for base in [format!("/media/{user}"), format!("/run/media/{user}")] {
        if user.is_empty() {
            break;
        }
        if let Ok(names) = zyrik::picker::list_dirs(std::path::Path::new(&base)) {
            out.extend(names.into_iter().map(|n| Place { kind: PlaceKind::Drive, path: PathBuf::from(&base).join(n) }));
        }
    }
    out
}

impl Platform for Linux {
    fn system_lang(&self) -> Option<String> {
        ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
    }

    fn claim_single_instance(&mut self) -> Result<(), Problem> {
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::SocketAddr;
        // абстрактный сокет: живёт, пока жив процесс, файла на диске нет
        let uid = unsafe { libc::getuid() };
        let addr = SocketAddr::from_abstract_name(format!("zyrik-{uid}").as_bytes()).map_err(os)?;
        match UnixListener::bind_addr(&addr) {
            Ok(l) => {
                self.guard = Some(l);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => Err(Problem::AlreadyRunning),
            Err(e) => Err(os(e)),
        }
    }

    fn show_tray(&mut self, tip: &str, menu: &[MenuItem]) -> Result<(), Problem> {
        // Трея может не быть (GNOME без расширения, голый X) — это не повод не работать:
        // горячая клавиша остаётся.
        match tray::spawn(self.tx.clone(), tip, menu) {
            Ok(h) => self.tray = Some(h),
            Err(e) => self.debug_log(&format!("значок в трее недоступен: {e}")),
        }
        Ok(())
    }

    fn set_tray_tip(&mut self, tip: &str) {
        if let Some(h) = &self.tray {
            tray::set_tip(h, tip);
        }
    }

    fn register_hotkey(&mut self, shot: Shot, key: Hotkey) -> Result<(), Problem> {
        let x = &self.x;
        let codes = x.keymap.lock().unwrap().codes(keys::hotkey_sym(key.key));
        let code = *codes.first().ok_or_else(|| Problem::Os(format!("на клавиатуре нет клавиши {key}")))?;
        let mods = keys::hotkey_mods(key.mods);
        let mut grabbed = Vec::new();
        for ignored in keys::IGNORED {
            let m = u16::from(mods) | ignored;
            let r = x
                .conn
                .grab_key(false, x.root, m.into(), code, GrabMode::ASYNC, GrabMode::ASYNC)
                .map_err(os)?
                .check();
            match r {
                Ok(()) => grabbed.push(m),
                Err(err) => {
                    for m in grabbed {
                        let _ = x.conn.ungrab_key(code, x.root, m.into());
                    }
                    return Err(match err {
                        ReplyError::X11Error(e) if e.error_kind == ErrorKind::Access => Problem::KeyBusy,
                        other => os(other),
                    });
                }
            }
        }
        x.hotkeys.lock().unwrap().push((code, u16::from(mods), shot));
        Ok(())
    }

    fn schedule(&mut self, command: Command, delay_ms: u32) {
        self.timers.push((Instant::now() + Duration::from_millis(u64::from(delay_ms)), command));
    }

    fn next_command(&mut self) -> Option<Command> {
        loop {
            let now = Instant::now();
            if let Some(i) = self.timers.iter().position(|(t, _)| *t <= now) {
                return Some(self.timers.remove(i).1);
            }
            let ev = match self.timers.iter().map(|(t, _)| *t - now).min() {
                Some(wait) => match self.rx.recv_timeout(wait) {
                    Ok(ev) => ev,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => return None,
                },
                None => self.rx.recv().ok()?,
            };
            match ev {
                Event::Cmd(c) => return Some(c),
                Event::Quit => return None,
            }
        }
    }

    fn capture(&mut self) -> Result<Frame, Problem> {
        let x = &self.x;
        let g = x.conn.get_geometry(x.root).map_err(os)?.reply().map_err(os)?;
        let (w, h) = (g.width, g.height);
        self.size = (w, h);
        let bpp = x.conn.setup().pixmap_formats.iter().find(|f| f.depth == x.depth).map(|f| f.bits_per_pixel);
        if bpp != Some(32) {
            return Err(Problem::Os(format!("глубина цвета {} бит не поддерживается", x.depth)));
        }
        let mut r = x
            .conn
            .get_image(ImageFormat::Z_PIXMAP, x.root, 0, 0, w, h, !0)
            .map_err(os)?
            .reply()
            .map_err(os)?;
        let msb = x.conn.setup().image_byte_order == ImageOrder::MSB_FIRST;
        let px: Vec<u32> = r
            .data
            .as_chunks::<4>().0.iter()
            .map(|b| {
                let b = [b[0], b[1], b[2], b[3]];
                0xFF00_0000 | if msb { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) }
            })
            .collect();
        zeroize::Zeroize::zeroize(&mut r.data);
        if px.len() != usize::from(w) * usize::from(h) {
            return Err(Problem::Os("GetImage: неожиданный размер".into()));
        }
        Ok(Frame { x: 0, y: 0, image: Image::from_pixels(u32::from(w), u32::from(h), px), monitors: monitors(x, w, h) })
    }

    fn show_overlay(&mut self, x0: i32, y0: i32, width: u32, height: u32) -> Result<(), Problem> {
        let x = &self.x;
        let aux = ConfigureWindowAux::new().x(x0).y(y0).width(width).height(height).stack_mode(StackMode::ABOVE);
        x.conn.configure_window(x.overlay, &aux).map_err(os)?;
        x.conn.map_window(x.overlay).map_err(os)?;
        x.shown.store(true, Ordering::Relaxed);
        // клавиатура и мышь — окну выделения; клавиша-триггер может ещё держать захват — повторяем
        let mut grabbed = false;
        for _ in 0..50 {
            let k = x
                .conn
                .grab_keyboard(false, x.overlay, CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)
                .map_err(os)?
                .reply()
                .map_err(os)?;
            if k.status == GrabStatus::SUCCESS {
                grabbed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !grabbed {
            // Клавиатуру держит оболочка (обзор GNOME, открытое меню панели): выделение было бы
            // под ней и не получало бы ни мыши, ни клавиш — не открываем его вовсе.
            self.hide_overlay();
            return Err(Problem::Os("клавиатура занята другой программой (открыт обзор или меню рабочего стола)".into()));
        }
        let mask = EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION;
        let _ = x.conn.grab_pointer(true, x.overlay, mask, GrabMode::ASYNC, GrabMode::ASYNC, NONE, NONE, CURRENT_TIME);
        let _ = x.conn.set_input_focus(InputFocus::PARENT, x.overlay, CURRENT_TIME);
        x.conn.flush().map_err(os)
    }

    fn present(&mut self, image: &Image) {
        let x = &self.x;
        let (w, h) = (image.width() as usize, image.height() as usize);
        let max = x.conn.maximum_request_bytes().saturating_sub(64);
        let rows = (max / (w * 4).max(1)).clamp(1, h.max(1));
        let msb = x.conn.setup().image_byte_order == ImageOrder::MSB_FIRST;
        let mut buf = Vec::with_capacity(rows * w * 4);
        for y in (0..h).step_by(rows) {
            let n = rows.min(h - y);
            buf.clear();
            for p in &image.pixels()[y * w..(y + n) * w] {
                buf.extend_from_slice(&if msb { p.to_be_bytes() } else { p.to_le_bytes() });
            }
            let _ = x.conn.put_image(ImageFormat::Z_PIXMAP, x.overlay, x.gc, w as u16, n as u16, 0, y as i16, 0, x.depth, &buf);
        }
        zeroize::Zeroize::zeroize(&mut buf);
        let _ = x.conn.flush();
    }

    fn set_cursor(&mut self, cursor: Cursor) {
        if self.cursor == Some(cursor) {
            return;
        }
        self.cursor = Some(cursor);
        let i = CURSORS.iter().position(|&c| c == cursor).unwrap_or(0);
        let aux = ChangeWindowAttributesAux::new().cursor(self.cursors[i]);
        let _ = self.x.conn.change_window_attributes(self.x.overlay, &aux);
        let _ = self.x.conn.flush();
    }

    fn hide_overlay(&mut self) {
        let x = &self.x;
        x.shown.store(false, Ordering::Relaxed);
        let _ = x.conn.ungrab_keyboard(CURRENT_TIME);
        let _ = x.conn.ungrab_pointer(CURRENT_TIME);
        // закрасить чёрным перед тем, как спрятать: композитор не должен держать кадр со снимком
        let (w, h) = self.size;
        let _ = x.conn.poly_fill_rectangle(x.overlay, x.gc, &[Rectangle { x: 0, y: 0, width: w, height: h }]);
        let _ = x.conn.unmap_window(x.overlay);
        let _ = x.conn.flush();
        self.cursor = None;
    }

    fn set_clipboard_image(&mut self, image: Image) -> Result<(), Problem> {
        let r = clipboard::set_image(&self.x, image);
        let _ = self.x.conn.flush();
        r
    }

    fn places(&self) -> Vec<Place> {
        places()
    }

    fn local_time(&self) -> Stamp {
        unsafe {
            let t = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&t, &mut tm);
            Stamp {
                year: (tm.tm_year + 1900) as u16,
                month: (tm.tm_mon + 1) as u8,
                day: tm.tm_mday as u8,
                hour: tm.tm_hour as u8,
                minute: tm.tm_min as u8,
                second: tm.tm_sec as u8,
            }
        }
    }

    fn message(&self, title: &str, text: &str, error: bool) {
        let _ = message(title, text, error);
    }

    fn debug_log(&self, text: &str) {
        eprintln!("zyrik: {text}");
    }
}

impl Drop for Linux {
    fn drop(&mut self) {
        if let Some(h) = self.tray.take() {
            let _ = h.shutdown();
        }
        clipboard::lost(&self.x);
    }
}

/// Сообщение: уведомление рабочего стола (org.freedesktop.Notifications) и текст в stderr.
///
/// Уведомление шлётся из отдельного потока и без ожидания: под GNOME вызов Notify из нашего
/// процесса мог не вернуться, и Зырик переставал отвечать. Возвращает приёмник, по которому
/// можно дождаться отправки (нужно перед выходом из программы).
pub fn message(title: &str, text: &str, error: bool) -> std::sync::mpsc::Receiver<()> {
    eprintln!("{title}: {text}");
    let (done, rx) = channel();
    if !tray::bus_is_local() {
        let _ = done.send(());
        return rx;
    }
    let (title, text) = (title.to_string(), text.to_string());
    std::thread::spawn(move || {
        if let Ok(bus) = zbus::blocking::Connection::session() {
            let hints: std::collections::HashMap<&str, zbus::zvariant::Value> =
                [("urgency", zbus::zvariant::Value::U8(if error { 2 } else { 1 }))].into_iter().collect();
            let body = (title.as_str(), 0u32, "", title.as_str(), text.as_str(), Vec::<&str>::new(), hints, -1i32);
            let msg = zbus::Message::method_call("/org/freedesktop/Notifications", "Notify")
                .and_then(|b| b.destination("org.freedesktop.Notifications"))
                .and_then(|b| b.interface("org.freedesktop.Notifications"))
                .and_then(|b| b.with_flags(zbus::message::Flags::NoReplyExpected))
                .and_then(|b| b.build(&body));
            if let Ok(msg) = msg {
                let _ = bus.send(&msg);
            }
        }
        let _ = done.send(());
    });
    rx
}
