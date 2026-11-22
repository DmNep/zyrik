//! Прослойка Linux Wayland (фаза 5). Ядро то же, здесь — протоколы Wayland на чистом Rust
//! (`wayland-client` со своей реализацией протокола, без libwayland; раскладки — `kbvm`).
//!
//! - Захват: `wlr-screencopy` (Sway, Hyprland, river…) в память через `memfd` — не файл на диске.
//! - Окно выделения: `wlr-layer-shell`, слой overlay, по поверхности на монитор; клавиатура —
//!   эксклюзивно. Размер в пикселях общего кадра, `wp_viewporter` растягивает до логического размера.
//! - Буфер: `ext-data-control` (KDE Plasma 6), `wlr-data-control` (wlroots) или, где их нет (GNOME),
//!   обычный `wl_data_device` — его можно занять, только пока окно выделения в фокусе.
//!   Снимок отдаётся по запросу, с `x-kde-passwordManagerHint`.
//! - Горячая клавиша: Wayland не даёт её программам — бинд в конфиге композитора на `zyrik --trigger`.
//!
//! Принцип I: соединение только с сокетом композитора (`$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`).

use std::collections::VecDeque;
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use kbvm::lookup::LookupTable;
use kbvm::xkb::diagnostic::WriteToLog;
use kbvm::{GroupIndex, Keycode, ModifierMask};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{
    wl_buffer::{self, WlBuffer},
    wl_compositor::WlCompositor,
    wl_data_device::{self, WlDataDevice},
    wl_data_device_manager::WlDataDeviceManager,
    wl_data_offer::WlDataOffer,
    wl_data_source::{self, WlDataSource},
    wl_keyboard::{self, WlKeyboard},
    wl_output::{self, WlOutput},
    wl_pointer::{self, WlPointer},
    wl_registry::WlRegistry,
    wl_seat::{self, WlSeat},
    wl_shm::{self, WlShm},
    wl_shm_pool::WlShmPool,
    wl_surface::WlSurface,
};
use wayland_client::{delegate_noop, event_created_child, Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::ExtDataControlManagerV1,
    ext_data_control_offer_v1::ExtDataControlOfferV1,
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1},
    wp_cursor_shape_manager_v1::WpCursorShapeManagerV1,
};
use wayland_protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter};
use wayland_protocols::xdg::shell::client::{
    xdg_surface::{self, XdgSurface},
    xdg_toplevel::{self, XdgToplevel},
    xdg_wm_base::{self, XdgWmBase},
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::ZwlrDataControlOfferV1,
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};
use wayland_protocols_wlr::screencopy::v1::client::{
    zwlr_screencopy_frame_v1::{self, ZwlrScreencopyFrameV1},
    zwlr_screencopy_manager_v1::ZwlrScreencopyManagerV1,
};
use zeroize::Zeroizing;

use zyrik::export::{encode_png, Stamp};
use zyrik::hotkey::Hotkey;
use zyrik::image::Image;
use zyrik::platform::{Command, Frame, MenuItem, Monitor, Place, Platform, Problem, Shot};
use zyrik::session::{Button, Cursor, Input, Key, Pt, Rect};

use super::{instance, keys, tray, Event};

/// Сколько ждать снимка монитора
const COPY_TIMEOUT: Duration = Duration::from_secs(3);
/// Запросы буфера в первые мс после того, как снимок туда положен, — это менеджеры буфера,
/// следящие за каждым изменением, а не вставка человеком: им не отдаём.
const WATCHER_WINDOW: Duration = Duration::from_millis(300);
/// После последней отданной вставки через столько очищаем буфер (Ctrl ещё могут держать
/// для повторной вставки; узнать, отпущен ли Ctrl, Wayland не даёт)
const CLEAR_AFTER: Duration = Duration::from_millis(1500);
/// Как часто проверять очередь команд от трея и `--trigger`, пока ждём событий Wayland
const IDLE_POLL: Duration = Duration::from_millis(100);

fn os<E: std::fmt::Display>(e: E) -> Problem {
    Problem::Os(e.to_string())
}

// ---------- общая память ----------

/// Кусок анонимной памяти (memfd) и wl_buffer поверх него. Затирается при удалении.
struct Shm {
    ptr: *mut u8,
    len: usize,
    width: u32,
    height: u32,
    stride: u32,
    pool: WlShmPool,
    buffer: WlBuffer,
    _fd: OwnedFd,
}

impl Shm {
    fn new(shm: &WlShm, qh: &QueueHandle<State>, w: u32, h: u32, stride: u32, format: wl_shm::Format, tag: usize) -> Result<Shm, Problem> {
        let len = (stride * h) as usize;
        let fd = unsafe {
            let raw = libc::memfd_create(c"zyrik".as_ptr(), libc::MFD_CLOEXEC);
            if raw < 0 {
                return Err(os(std::io::Error::last_os_error()));
            }
            OwnedFd::from_raw_fd(raw)
        };
        if unsafe { libc::ftruncate(fd.as_raw_fd(), len as libc::off_t) } != 0 {
            return Err(os(std::io::Error::last_os_error()));
        }
        let ptr = unsafe {
            libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd.as_raw_fd(), 0)
        };
        if ptr == libc::MAP_FAILED {
            return Err(os(std::io::Error::last_os_error()));
        }
        let pool = shm.create_pool(fd.as_fd(), len as i32, qh, ());
        let buffer = pool.create_buffer(0, w as i32, h as i32, stride as i32, format, qh, tag);
        Ok(Shm { ptr: ptr.cast(), len, width: w, height: h, stride, pool, buffer, _fd: fd })
    }

    fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for Shm {
    fn drop(&mut self) {
        self.bytes_mut().fill(0);
        self.buffer.destroy();
        self.pool.destroy();
        unsafe {
            libc::munmap(self.ptr.cast(), self.len);
        }
    }
}

// ---------- состояние ----------

#[derive(Default)]
struct Output {
    wl: Option<WlOutput>,
    xdg: Option<ZxdgOutputV1>,
    /// Логическое положение и размер (точки)
    logical: Rect,
    scale: i32,
    /// Кусок общего кадра, пиксели
    px: Rect,
}

/// Роль окна выделения: слой поверх всего (wlr-layer-shell) или, где его нет (GNOME),
/// полноэкранное окно.
enum Role {
    Layer(ZwlrLayerSurfaceV1),
    Toplevel(XdgSurface, XdgToplevel),
}

/// Поверхность выделения на одном мониторе.
struct OverlaySurface {
    surface: WlSurface,
    role: Role,
    viewport: Option<WpViewport>,
    configured: bool,
    buffers: Vec<(Shm, bool)>,
}

#[derive(Default)]
struct Copy {
    format: Option<(wl_shm::Format, u32, u32, u32)>,
    buffer_done: bool,
    ready: bool,
    failed: bool,
    y_invert: bool,
}

/// Источник данных буфера — по тому, что умеет композитор.
#[derive(Clone, PartialEq)]
enum Source {
    Ext(ExtDataControlSourceV1),
    Wlr(ZwlrDataControlSourceV1),
    Core(WlDataSource),
}

impl Source {
    fn offer(&self, mime: &str) {
        match self {
            Source::Ext(s) => s.offer(mime.into()),
            Source::Wlr(s) => s.offer(mime.into()),
            Source::Core(s) => s.offer(mime.into()),
        }
    }

    fn destroy(&self) {
        match self {
            Source::Ext(s) => s.destroy(),
            Source::Wlr(s) => s.destroy(),
            Source::Core(s) => s.destroy(),
        }
    }
}

/// Как класть в буфер.
enum ClipDevice {
    Ext(ExtDataControlManagerV1, ExtDataControlDeviceV1),
    Wlr(ZwlrDataControlManagerV1, ZwlrDataControlDeviceV1),
    Core(WlDataDeviceManager, WlDataDevice),
    None,
}

struct Clip {
    image: Option<Image>,
    png: Option<Zeroizing<Vec<u8>>>,
    source: Option<Source>,
    set_at: Instant,
    /// Когда очистить буфер (после отданной вставки)
    clear_at: Option<Instant>,
}

pub struct State {
    queue: VecDeque<Command>,
    outputs: Vec<Output>,
    overlays: Vec<OverlaySurface>,
    copies: Vec<Copy>,
    shift: bool,
    mods: (u32, u32, u32, u32),
    lookup: Option<LookupTable>,
    pointer_on: Option<usize>,
    pointer_serial: u32,
    /// Последняя точка курсора: кнопкам Wayland координат не присылает
    pointer: Pt,
    /// Номер последнего события ввода — для `wl_data_device.set_selection`
    input_serial: u32,
    last_click: Option<(Instant, Pt)>,
    wheel_acc: f64,
    scale: f64,
    clip: Clip,
    /// Мышь и клавиатура появляются по событию `capabilities` (и могут появиться позже)
    wl_pointer: Option<WlPointer>,
    wl_keyboard: Option<WlKeyboard>,
    cursor_shape: Option<WpCursorShapeManagerV1>,
    cursor_device: Option<WpCursorShapeDeviceV1>,
}

pub struct Wayland {
    _conn: Connection,
    queue: EventQueue<State>,
    qh: QueueHandle<State>,
    state: State,
    compositor: WlCompositor,
    shm: WlShm,
    layer_shell: Option<ZwlrLayerShellV1>,
    wm_base: Option<XdgWmBase>,
    screencopy: Option<ZwlrScreencopyManagerV1>,
    clip_device: ClipDevice,
    viewporter: Option<WpViewporter>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    timers: Vec<(Instant, Command)>,
    tray: Option<ksni::blocking::Handle<tray::ZTray>>,
    cursor: Option<Cursor>,
}

impl Wayland {
    /// `None` — не Wayland или композитор без нужных протоколов (тогда пробуем X11).
    pub fn new() -> Result<Wayland, String> {
        std::env::var_os("WAYLAND_DISPLAY").ok_or("WAYLAND_DISPLAY не задан")?;
        let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
        let (globals, mut queue) = registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let compositor: WlCompositor = globals.bind(&qh, 4..=6, ()).map_err(|e| format!("wl_compositor: {e}"))?;
        let shm: WlShm = globals.bind(&qh, 1..=1, ()).map_err(|e| format!("wl_shm: {e}"))?;
        let layer_shell: Option<ZwlrLayerShellV1> = globals.bind(&qh, 1..=4, ()).ok();
        let wm_base: Option<XdgWmBase> = globals.bind(&qh, 1..=5, ()).ok();
        if layer_shell.is_none() && wm_base.is_none() {
            return Err("композитор не даёт ни wlr-layer-shell, ни xdg-shell".into());
        }
        let seat: WlSeat = globals.bind(&qh, 1..=7, ()).map_err(|e| format!("wl_seat: {e}"))?;
        let screencopy: Option<ZwlrScreencopyManagerV1> = globals.bind(&qh, 1..=3, ()).ok();
        let ext_control: Option<ExtDataControlManagerV1> = globals.bind(&qh, 1..=1, ()).ok();
        let wlr_control: Option<ZwlrDataControlManagerV1> = globals.bind(&qh, 1..=2, ()).ok();
        let core_dd: Option<WlDataDeviceManager> = globals.bind(&qh, 3..=3, ()).ok();
        let viewporter: Option<WpViewporter> = globals.bind(&qh, 1..=1, ()).ok();
        let xdg_outputs: Option<ZxdgOutputManagerV1> = globals.bind(&qh, 2..=3, ()).ok();
        let cursor_shape: Option<WpCursorShapeManagerV1> = globals.bind(&qh, 1..=1, ()).ok();

        let mut state = State {
            queue: VecDeque::new(),
            outputs: Vec::new(),
            overlays: Vec::new(),
            copies: Vec::new(),
            shift: false,
            mods: (0, 0, 0, 0),
            lookup: None,
            pointer_on: None,
            pointer_serial: 0,
            pointer: Pt { x: 0, y: 0 },
            input_serial: 0,
            last_click: None,
            wheel_acc: 0.0,
            scale: 1.0,
            clip: Clip { image: None, png: None, source: None, set_at: Instant::now(), clear_at: None },
            wl_pointer: None,
            wl_keyboard: None,
            cursor_shape,
            cursor_device: None,
        };
        let list: Vec<(u32, u32)> = globals.contents().with_list(|l| {
            l.iter().filter(|g| g.interface == "wl_output").map(|g| (g.name, g.version)).collect()
        });
        for (i, (name, version)) in list.into_iter().enumerate() {
            let wl: WlOutput = globals.registry().bind(name, version.min(4), &qh, i);
            let xdg = xdg_outputs.as_ref().map(|m| m.get_xdg_output(&wl, &qh, i));
            state.outputs.push(Output { wl: Some(wl), xdg, scale: 1, ..Output::default() });
        }
        // мышь и клавиатура — по событию capabilities у wl_seat
        let clip_device = if let Some(m) = ext_control {
            let d = m.get_data_device(&seat, &qh, ());
            ClipDevice::Ext(m, d)
        } else if let Some(m) = wlr_control {
            let d = m.get_data_device(&seat, &qh, ());
            ClipDevice::Wlr(m, d)
        } else if let Some(m) = core_dd {
            let d = m.get_data_device(&seat, &qh, ());
            ClipDevice::Core(m, d)
        } else {
            ClipDevice::None
        };
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        if state.outputs.is_empty() {
            return Err("нет мониторов".into());
        }

        let (tx, rx) = channel();
        Ok(Wayland {
            _conn: conn,
            queue,
            qh,
            state,
            compositor,
            shm,
            layer_shell,
            wm_base,
            screencopy,
            clip_device,
            viewporter,
            tx,
            rx,
            timers: Vec::new(),
            tray: None,
            cursor: None,
        })
    }

    /// Дождаться событий Wayland (не дольше `wait`) и разобрать их.
    fn pump(&mut self, wait: Duration) -> Result<(), Problem> {
        self.queue.flush().map_err(os)?;
        if let Some(guard) = self.queue.prepare_read() {
            let fd = guard.connection_fd().as_raw_fd();
            let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
            let ms = wait.as_millis().min(i32::MAX as u128) as i32;
            let n = unsafe { libc::poll(&mut pfd, 1, ms) };
            if n > 0 {
                guard.read().map_err(os)?;
            }
        }
        self.queue.dispatch_pending(&mut self.state).map_err(os)?;
        Ok(())
    }

    /// Раскладка мониторов: логические координаты × наибольший масштаб.
    fn layout(&mut self) -> (u32, u32) {
        let s = self.state.scale;
        let min_x = self.state.outputs.iter().map(|o| o.logical.x).min().unwrap_or(0);
        let min_y = self.state.outputs.iter().map(|o| o.logical.y).min().unwrap_or(0);
        let r = |v: i32| (f64::from(v) * s).round() as i32;
        for o in &mut self.state.outputs {
            o.px = Rect { x: r(o.logical.x - min_x), y: r(o.logical.y - min_y), w: r(o.logical.w), h: r(o.logical.h) };
        }
        let w = self.state.outputs.iter().map(|o| o.px.right()).max().unwrap_or(1).max(1);
        let h = self.state.outputs.iter().map(|o| o.px.bottom()).max().unwrap_or(1).max(1);
        (w as u32, h as u32)
    }

    /// Снять один монитор через wlr-screencopy.
    fn copy_output(&mut self, i: usize) -> Result<(Shm, bool), Problem> {
        let mgr = self.screencopy.clone().ok_or_else(|| Problem::Os("композитор не даёт снимать экран (нет wlr-screencopy)".into()))?;
        let output = self.state.outputs[i].wl.clone().ok_or_else(|| Problem::Os("монитор пропал".into()))?;
        self.state.copies.resize_with(self.state.outputs.len(), Copy::default);
        self.state.copies[i] = Copy::default();
        let frame = mgr.capture_output(0, &output, &self.qh, i);
        let deadline = Instant::now() + COPY_TIMEOUT;
        let v3 = mgr.version() >= 3;
        loop {
            let c = &self.state.copies[i];
            if c.failed {
                frame.destroy();
                return Err(Problem::Os("screencopy: композитор отказал".into()));
            }
            if c.format.is_some() && (!v3 || c.buffer_done) {
                break;
            }
            if Instant::now() > deadline {
                frame.destroy();
                return Err(Problem::Os("screencopy: нет ответа".into()));
            }
            self.pump(Duration::from_millis(50))?;
        }
        let (format, w, h, stride) = self.state.copies[i].format.unwrap();
        let shm = Shm::new(&self.shm, &self.qh, w, h, stride, format, usize::MAX)?;
        frame.copy(&shm.buffer);
        loop {
            let c = &self.state.copies[i];
            if c.ready {
                break;
            }
            if c.failed || Instant::now() > deadline {
                frame.destroy();
                return Err(Problem::Os("screencopy: снимок не получился".into()));
            }
            self.pump(Duration::from_millis(50))?;
        }
        frame.destroy();
        Ok((shm, self.state.copies[i].y_invert))
    }

    /// Без screencopy: KDE — KWin ScreenShot2 (через трубу, без файла); не вышло или не KDE —
    /// портал (файл читается и сразу затирается, см. `portal`).
    fn capture_portal(&mut self) -> Result<Frame, Problem> {
        // только отладочная сборка: кадр из файла — проверка окна, ввода и буфера там, где
        // снимок экрана в контейнере не снять (KWin без GPU)
        #[cfg(debug_assertions)]
        if let Some(p) = std::env::var_os("ZYRIK_TEST_FRAME") {
            let data = std::fs::read(p).map_err(os)?;
            let img = super::portal::decode(&data)?;
            self.state.scale = 1.0;
            let _ = self.layout();
            let monitors = self.state.outputs.iter().map(|o| Monitor { rect: o.px, scale: 100 }).collect();
            return Ok(Frame { x: 0, y: 0, image: img, monitors });
        }
        let kde = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("KDE")));
        let img = match kde.then(super::kwin::capture) {
            Some(Ok(img)) => img,
            Some(Err(super::kwin::Fail::NotAuthorized)) => {
                self.debug_log("KWin не дал снимок без файла: нет zyrik.desktop с X-KDE-DBUS-Restricted-Interfaces (см. README) — пробую портал");
                super::portal::screenshot()?
            }
            Some(Err(super::kwin::Fail::Other(e))) => {
                self.debug_log(&format!("{e:?} — пробую портал"));
                super::portal::screenshot()?
            }
            None => super::portal::screenshot()?,
        };
        let lw = self.state.outputs.iter().map(|o| o.logical.right()).max().unwrap_or(1)
            - self.state.outputs.iter().map(|o| o.logical.x).min().unwrap_or(0);
        self.state.scale = (f64::from(img.width()) / f64::from(lw.max(1))).max(1.0);
        let (w, h) = self.layout();
        let img = if (img.width(), img.height()) == (w, h) {
            img
        } else {
            // портал отдал другой размер — подгоняем ближайшим пикселем
            let mut px = vec![0xFF00_0000u32; (w * h) as usize];
            for y in 0..h {
                let sy = (u64::from(y) * u64::from(img.height()) / u64::from(h)) as u32;
                for x in 0..w {
                    let sx = (u64::from(x) * u64::from(img.width()) / u64::from(w)) as u32;
                    px[(y * w + x) as usize] = img.get(sx, sy);
                }
            }
            Image::from_pixels(w, h, px)
        };
        let scale = (self.state.scale * 100.0).round() as u32;
        let monitors = self.state.outputs.iter().map(|o| Monitor { rect: o.px, scale }).collect();
        Ok(Frame { x: 0, y: 0, image: img, monitors })
    }

    fn free_buffer(&mut self, i: usize) -> Result<usize, Problem> {
        let px = self.state.outputs[i].px;
        let o = &mut self.state.overlays[i];
        if let Some(b) = o.buffers.iter().position(|(_, busy)| !busy) {
            return Ok(b);
        }
        let tag = i * 16 + o.buffers.len();
        let shm = Shm::new(&self.shm, &self.qh, px.w as u32, px.h as u32, px.w as u32 * 4, wl_shm::Format::Xrgb8888, tag)?;
        o.buffers.push((shm, false));
        Ok(o.buffers.len() - 1)
    }

    fn clear_clipboard(&mut self) {
        if let Some(src) = self.state.clip.source.take() {
            // источник, который сейчас в буфере, уничтожен — композитор очищает буфер
            src.destroy();
        }
        wipe(&mut self.state.clip);
    }
}

fn wipe(c: &mut Clip) {
    if let Some(mut img) = c.image.take() {
        img.wipe();
    }
    c.png = None;
    c.clear_at = None;
}

/// Пиксель кадра screencopy → 0xFFRRGGBB.
fn pixel(format: wl_shm::Format, b: &[u8]) -> u32 {
    let v = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let rgb = match format {
        wl_shm::Format::Abgr8888 | wl_shm::Format::Xbgr8888 => {
            ((v & 0xFF) << 16) | (v & 0xFF00) | ((v >> 16) & 0xFF)
        }
        _ => v & 0x00FF_FFFF,
    };
    0xFF00_0000 | rgb
}

impl Platform for Wayland {
    fn system_lang(&self) -> Option<String> {
        super::env_lang()
    }

    fn claim_single_instance(&mut self) -> Result<(), Problem> {
        instance::claim(self.tx.clone())
    }

    fn show_tray(&mut self, tip: &str, menu: &[MenuItem]) -> Result<(), Problem> {
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

    /// Глобальных клавиш Wayland программам не даёт: клавишу задают в конфиге композитора.
    fn register_hotkey(&mut self, _shot: Shot, _key: Hotkey) -> Result<(), Problem> {
        self.debug_log("Wayland: горячую клавишу задайте в конфиге композитора на `zyrik --trigger`");
        Ok(())
    }

    fn schedule(&mut self, command: Command, delay_ms: u32) {
        self.timers.push((Instant::now() + Duration::from_millis(u64::from(delay_ms)), command));
    }

    fn next_command(&mut self) -> Option<Command> {
        loop {
            if let Some(c) = self.state.queue.pop_front() {
                return Some(c);
            }
            match self.rx.try_recv() {
                Ok(Event::Cmd(c)) => return Some(c),
                Ok(Event::Quit) | Err(TryRecvError::Disconnected) => return None,
                Err(TryRecvError::Empty) => {}
            }
            let now = Instant::now();
            if let Some(i) = self.timers.iter().position(|(t, _)| *t <= now) {
                return Some(self.timers.remove(i).1);
            }
            if self.state.clip.clear_at.is_some_and(|t| t <= now) {
                self.clear_clipboard();
            }
            let mut wait = IDLE_POLL;
            for t in self.timers.iter().map(|(t, _)| *t).chain(self.state.clip.clear_at) {
                wait = wait.min(t.saturating_duration_since(now));
            }
            if self.pump(wait).is_err() {
                return None;
            }
        }
    }

    fn capture(&mut self) -> Result<Frame, Problem> {
        if self.screencopy.is_none() {
            return self.capture_portal();
        }
        // масштаб — по первому снимку каждого монитора: пикселей на логическую точку
        let mut shots = Vec::new();
        for i in 0..self.state.outputs.len() {
            shots.push(self.copy_output(i)?);
        }
        let scale = shots
            .iter()
            .zip(&self.state.outputs)
            .map(|((s, _), o)| f64::from(s.width) / f64::from(o.logical.w.max(1)))
            .fold(1.0, f64::max);
        self.state.scale = scale;
        let (w, h) = self.layout();
        let mut px = vec![0xFF00_0000u32; (w * h) as usize];
        for (i, ((shm, invert), o)) in shots.iter().zip(&self.state.outputs).enumerate() {
            let format = self.state.copies.get(i).and_then(|c| c.format).map_or(wl_shm::Format::Xrgb8888, |f| f.0);
            let bytes = shm.bytes();
            // кадр монитора → его кусок общего кадра (при другом масштабе — ближайший пиксель)
            for y in 0..o.px.h {
                let sy = (u64::from(y as u32) * u64::from(shm.height) / o.px.h.max(1) as u64) as u32;
                let sy = if *invert { shm.height - 1 - sy } else { sy };
                let row = (sy * shm.stride) as usize;
                let at = ((o.px.y + y) as u32 * w + o.px.x as u32) as usize;
                for x in 0..o.px.w {
                    let sx = (u64::from(x as u32) * u64::from(shm.width) / o.px.w.max(1) as u64) as usize;
                    px[at + x as usize] = pixel(format, &bytes[row + sx * 4..row + sx * 4 + 4]);
                }
            }
        }
        drop(shots); // затирается в Drop
        let monitors = self
            .state
            .outputs
            .iter()
            .map(|o| Monitor { rect: o.px, scale: (scale * 100.0).round() as u32 })
            .collect();
        Ok(Frame { x: 0, y: 0, image: Image::from_pixels(w, h, px), monitors })
    }

    fn show_overlay(&mut self, _x: i32, _y: i32, _w: u32, _h: u32) -> Result<(), Problem> {
        self.state.overlays.clear();
        for i in 0..self.state.outputs.len() {
            let out = &self.state.outputs[i];
            let surface = self.compositor.create_surface(&self.qh, ());
            let role = match (&self.layer_shell, &self.wm_base) {
                (Some(ls), _) => {
                    let layer = ls.get_layer_surface(&surface, out.wl.as_ref(), Layer::Overlay, "zyrik".into(), &self.qh, i);
                    layer.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
                    layer.set_exclusive_zone(-1);
                    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
                    Role::Layer(layer)
                }
                (None, Some(wm)) => {
                    let xs = wm.get_xdg_surface(&surface, &self.qh, i);
                    let top = xs.get_toplevel(&self.qh, i);
                    top.set_title("Zyrik".into());
                    top.set_app_id("zyrik".into());
                    top.set_fullscreen(out.wl.as_ref());
                    Role::Toplevel(xs, top)
                }
                (None, None) => unreachable!("проверено в new"),
            };
            let viewport = self.viewporter.as_ref().map(|v| {
                let vp = v.get_viewport(&surface, &self.qh, ());
                vp.set_destination(out.logical.w, out.logical.h);
                vp
            });
            if viewport.is_none() {
                surface.set_buffer_scale(self.state.scale.round() as i32);
            }
            surface.commit();
            self.state.overlays.push(OverlaySurface { surface, role, viewport, configured: false, buffers: Vec::new() });
        }
        let deadline = Instant::now() + COPY_TIMEOUT;
        while self.state.overlays.iter().any(|o| !o.configured) {
            if Instant::now() > deadline {
                self.hide_overlay();
                return Err(Problem::Os("композитор не показал окно выделения".into()));
            }
            self.pump(Duration::from_millis(20))?;
        }
        Ok(())
    }

    fn present(&mut self, image: &Image) {
        let w = image.width() as usize;
        for i in 0..self.state.overlays.len() {
            let Ok(b) = self.free_buffer(i) else { continue };
            let px = self.state.outputs[i].px;
            let o = &mut self.state.overlays[i];
            let (shm, busy) = &mut o.buffers[b];
            let dst = shm.bytes_mut();
            for y in 0..px.h as usize {
                let at = (px.y as usize + y) * w + px.x as usize;
                for (x, p) in image.pixels()[at..at + px.w as usize].iter().enumerate() {
                    let d = (y * px.w as usize + x) * 4;
                    dst[d..d + 4].copy_from_slice(&p.to_le_bytes());
                }
            }
            *busy = true;
            o.surface.attach(Some(&o.buffers[b].0.buffer), 0, 0);
            o.surface.damage_buffer(0, 0, px.w, px.h);
            o.surface.commit();
        }
        let _ = self.queue.flush();
    }

    fn set_cursor(&mut self, cursor: Cursor) {
        if self.cursor == Some(cursor) {
            return;
        }
        self.cursor = Some(cursor);
        let shape = match cursor {
            Cursor::Cross => Shape::Crosshair,
            Cursor::Arrow => Shape::Default,
            Cursor::Text => Shape::Text,
            Cursor::Move => Shape::Move,
            Cursor::ResizeNwse => Shape::NwseResize,
            Cursor::ResizeNesw => Shape::NeswResize,
            Cursor::ResizeNs => Shape::NsResize,
            Cursor::ResizeWe => Shape::EwResize,
        };
        if let Some(d) = &self.state.cursor_device {
            d.set_shape(self.state.pointer_serial, shape);
            let _ = self.queue.flush();
        }
    }

    fn hide_overlay(&mut self) {
        for o in self.state.overlays.drain(..) {
            match o.role {
                Role::Layer(l) => l.destroy(),
                Role::Toplevel(xs, top) => {
                    top.destroy();
                    xs.destroy();
                }
            }
            if let Some(v) = o.viewport {
                v.destroy();
            }
            o.surface.destroy();
            // буферы затираются в Drop
        }
        self.cursor = None;
        let _ = self.queue.roundtrip(&mut self.state);
    }

    /// Без data-control (GNOME) буфер можно занять, только пока окно выделения в фокусе.
    fn clipboard_needs_focus(&self) -> bool {
        matches!(self.clip_device, ClipDevice::Core(..))
    }

    fn set_clipboard_image(&mut self, image: Image) -> Result<(), Problem> {
        self.clear_clipboard();
        let qh = &self.qh;
        let src = match &self.clip_device {
            ClipDevice::Ext(m, _) => Source::Ext(m.create_data_source(qh, ())),
            ClipDevice::Wlr(m, _) => Source::Wlr(m.create_data_source(qh, ())),
            ClipDevice::Core(m, _) => Source::Core(m.create_data_source(qh, ())),
            ClipDevice::None => return Err(Problem::Os("композитор не даёт буфер обмена".into())),
        };
        src.offer("image/png");
        src.offer("x-kde-passwordManagerHint");
        match (&self.clip_device, &src) {
            (ClipDevice::Ext(_, d), Source::Ext(s)) => d.set_selection(Some(s)),
            (ClipDevice::Wlr(_, d), Source::Wlr(s)) => d.set_selection(Some(s)),
            (ClipDevice::Core(_, d), Source::Core(s)) => d.set_selection(Some(s), self.state.input_serial),
            _ => {}
        }
        let c = &mut self.state.clip;
        c.image = Some(image);
        c.source = Some(src);
        c.set_at = Instant::now();
        self.queue.flush().map_err(os)?;
        Ok(())
    }

    fn places(&self) -> Vec<Place> {
        super::places()
    }

    fn local_time(&self) -> Stamp {
        super::local_time()
    }

    fn message(&self, title: &str, text: &str, error: bool) {
        let _ = super::message(title, text, error);
    }

    fn debug_log(&self, text: &str) {
        eprintln!("zyrik: {text}");
    }
}

impl Drop for Wayland {
    fn drop(&mut self) {
        if let Some(h) = self.tray.take() {
            let _ = h.shutdown();
        }
        self.clear_clipboard();
    }
}

// ---------- события ----------

impl State {
    fn input(&mut self, i: Input) {
        self.queue.push_back(Command::Overlay(i));
    }

    fn point(&self, x: f64, y: f64) -> Pt {
        let Some(o) = self.pointer_on.and_then(|i| self.outputs.get(i)) else { return Pt { x: 0, y: 0 } };
        Pt { x: o.px.x + (x * self.scale).round() as i32, y: o.px.y + (y * self.scale).round() as i32 }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &WlRegistry, _: <WlRegistry as Proxy>::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

delegate_noop!(State: ignore WlCompositor);
delegate_noop!(State: ignore WlShm);
delegate_noop!(State: ignore WlShmPool);
delegate_noop!(State: ignore WlSurface);
delegate_noop!(State: ignore ZwlrLayerShellV1);
delegate_noop!(State: ignore ZwlrScreencopyManagerV1);
delegate_noop!(State: ignore ZwlrDataControlManagerV1);
delegate_noop!(State: ignore ZwlrDataControlOfferV1);
delegate_noop!(State: ignore ExtDataControlManagerV1);
delegate_noop!(State: ignore ExtDataControlOfferV1);
delegate_noop!(State: ignore WlDataDeviceManager);
delegate_noop!(State: ignore WlDataOffer);
delegate_noop!(State: ignore WpViewporter);
delegate_noop!(State: ignore WpViewport);
delegate_noop!(State: ignore ZxdgOutputManagerV1);
delegate_noop!(State: ignore WpCursorShapeManagerV1);
delegate_noop!(State: ignore WpCursorShapeDeviceV1);

impl Dispatch<WlBuffer, usize> for State {
    fn event(state: &mut Self, _: &WlBuffer, event: wl_buffer::Event, tag: &usize, _: &Connection, _: &QueueHandle<Self>) {
        if let wl_buffer::Event::Release = event {
            let (i, b) = (*tag / 16, *tag % 16);
            if let Some((_, busy)) = state.overlays.get_mut(i).and_then(|o| o.buffers.get_mut(b)) {
                *busy = false;
            }
        }
    }
}

impl Dispatch<WlOutput, usize> for State {
    fn event(state: &mut Self, _: &WlOutput, event: wl_output::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let Some(o) = state.outputs.get_mut(*i) else { return };
        match event {
            wl_output::Event::Scale { factor } => o.scale = factor,
            // без xdg-output: размер в пикселях режима, делённый на масштаб
            wl_output::Event::Mode { width, height, .. } if o.xdg.is_none() => {
                let s = o.scale.max(1);
                o.logical.w = width / s;
                o.logical.h = height / s;
            }
            wl_output::Event::Geometry { x, y, .. } if o.xdg.is_none() => {
                o.logical.x = x;
                o.logical.y = y;
            }
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputV1, usize> for State {
    fn event(state: &mut Self, _: &ZxdgOutputV1, event: zxdg_output_v1::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let Some(o) = state.outputs.get_mut(*i) else { return };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                o.logical.x = x;
                o.logical.y = y;
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                o.logical.w = width;
                o.logical.h = height;
            }
            _ => {}
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(state: &mut Self, seat: &WlSeat, event: wl_seat::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        let wl_seat::Event::Capabilities { capabilities: WEnum::Value(caps) } = event else { return };
        if caps.contains(wl_seat::Capability::Pointer) && state.wl_pointer.is_none() {
            let p = seat.get_pointer(qh, ());
            state.cursor_device = state.cursor_shape.as_ref().map(|m| m.get_pointer(&p, qh, ()));
            state.wl_pointer = Some(p);
        }
        if caps.contains(wl_seat::Capability::Keyboard) && state.wl_keyboard.is_none() {
            state.wl_keyboard = Some(seat.get_keyboard(qh, ()));
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, usize> for State {
    fn event(state: &mut Self, ls: &ZwlrLayerSurfaceV1, event: zwlr_layer_surface_v1::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        match event {
            zwlr_layer_surface_v1::Event::Configure { serial, .. } => {
                ls.ack_configure(serial);
                if let Some(o) = state.overlays.get_mut(*i) {
                    o.configured = true;
                }
                state.input(Input::Repaint);
            }
            zwlr_layer_surface_v1::Event::Closed => state.input(Input::Key { key: Key::Esc, shift: false, ctrl: false }),
            _ => {}
        }
    }
}

impl Dispatch<XdgWmBase, ()> for State {
    fn event(_: &mut Self, wm: &XdgWmBase, event: xdg_wm_base::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, usize> for State {
    fn event(state: &mut Self, xs: &XdgSurface, event: xdg_surface::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_surface::Event::Configure { serial } = event {
            xs.ack_configure(serial);
            if let Some(o) = state.overlays.get_mut(*i) {
                o.configured = true;
            }
            state.input(Input::Repaint);
        }
    }
}

impl Dispatch<XdgToplevel, usize> for State {
    fn event(state: &mut Self, _: &XdgToplevel, event: xdg_toplevel::Event, _: &usize, _: &Connection, _: &QueueHandle<Self>) {
        if let xdg_toplevel::Event::Close = event {
            state.input(Input::Key { key: Key::Esc, shift: false, ctrl: false });
        }
    }
}

impl Dispatch<ZwlrScreencopyFrameV1, usize> for State {
    fn event(state: &mut Self, _: &ZwlrScreencopyFrameV1, event: zwlr_screencopy_frame_v1::Event, i: &usize, _: &Connection, _: &QueueHandle<Self>) {
        let Some(c) = state.copies.get_mut(*i) else { return };
        match event {
            zwlr_screencopy_frame_v1::Event::Buffer { format: WEnum::Value(f), width, height, stride } => {
                // берём первый 32-битный формат, который умеем разбирать
                let ok = matches!(f, wl_shm::Format::Xrgb8888 | wl_shm::Format::Argb8888 | wl_shm::Format::Xbgr8888 | wl_shm::Format::Abgr8888);
                if ok && c.format.is_none() {
                    c.format = Some((f, width, height, stride));
                }
            }
            zwlr_screencopy_frame_v1::Event::BufferDone => c.buffer_done = true,
            zwlr_screencopy_frame_v1::Event::Flags { flags: WEnum::Value(f) } => {
                c.y_invert = f.contains(zwlr_screencopy_frame_v1::Flags::YInvert);
            }
            zwlr_screencopy_frame_v1::Event::Ready { .. } => c.ready = true,
            zwlr_screencopy_frame_v1::Event::Failed => c.failed = true,
            _ => {}
        }
    }
}

impl Dispatch<WlPointer, ()> for State {
    fn event(state: &mut Self, _: &WlPointer, event: wl_pointer::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if std::env::var_os("ZYRIK_DEBUG_INPUT").is_some() && !matches!(event, wl_pointer::Event::Motion { .. } | wl_pointer::Event::Frame) {
            eprintln!("zyrik: мышь {event:?} (над монитором {:?})", state.pointer_on);
        }
        match event {
            wl_pointer::Event::Enter { serial, surface, surface_x, surface_y } => {
                state.pointer_serial = serial;
                state.pointer_on = state.overlays.iter().position(|o| o.surface == surface);
                state.moved(surface_x, surface_y);
            }
            wl_pointer::Event::Leave { .. } => state.pointer_on = None,
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => state.moved(surface_x, surface_y),
            wl_pointer::Event::Button { serial, button, state: WEnum::Value(bs), .. } => {
                state.input_serial = serial;
                let p = state.last_move();
                let pressed = bs == wl_pointer::ButtonState::Pressed;
                match (button, pressed) {
                    (0x110, true) => {
                        let now = Instant::now();
                        let double = state.last_click.is_some_and(|(t, q)| {
                            now - t < Duration::from_millis(400) && (q.x - p.x).abs() <= 4 && (q.y - p.y).abs() <= 4
                        });
                        state.last_click = if double { None } else { Some((now, p)) };
                        state.input(if double { Input::DoubleClick(p) } else { Input::Down(Button::Left, p) });
                    }
                    (0x110, false) => state.input(Input::Up(Button::Left, p)),
                    (0x111, true) => state.input(Input::Down(Button::Right, p)),
                    (0x111, false) => state.input(Input::Up(Button::Right, p)),
                    _ => {}
                }
            }
            wl_pointer::Event::AxisDiscrete { axis: WEnum::Value(wl_pointer::Axis::VerticalScroll), discrete } => {
                let p = state.last_move();
                state.wheel_acc = 0.0;
                state.input(Input::Wheel(-discrete, p));
            }
            wl_pointer::Event::Axis { axis: WEnum::Value(wl_pointer::Axis::VerticalScroll), value, .. } => {
                // тачпад без «щелчков»: копим, щелчок — каждые 15 единиц
                state.wheel_acc += value;
                let n = (state.wheel_acc / 15.0).trunc();
                if n != 0.0 {
                    state.wheel_acc -= n * 15.0;
                    let p = state.last_move();
                    state.input(Input::Wheel(-(n as i32), p));
                }
            }
            _ => {}
        }
    }
}

impl State {
    fn last_move(&self) -> Pt {
        self.pointer
    }

    fn moved(&mut self, x: f64, y: f64) {
        let p = self.point(x, y);
        self.pointer = p;
        self.input(Input::Move(p));
    }
}

impl Dispatch<WlKeyboard, ()> for State {
    fn event(state: &mut Self, _: &WlKeyboard, event: wl_keyboard::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_keyboard::Event::Keymap { format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1), fd, size } => {
                state.lookup = load_keymap(&fd, size as usize);
            }
            wl_keyboard::Event::Modifiers { mods_depressed, mods_latched, mods_locked, group, .. } => {
                if std::env::var_os("ZYRIK_DEBUG_INPUT").is_some() {
                    eprintln!("zyrik: модификаторы {mods_depressed:x}/{mods_latched:x}/{mods_locked:x} группа {group}");
                }
                state.mods = (mods_depressed, mods_latched, mods_locked, group);
                let shift = (mods_depressed | mods_latched) & 1 != 0;
                if shift != state.shift {
                    state.shift = shift;
                    state.input(Input::Shift(shift));
                }
            }
            wl_keyboard::Event::Key { serial, key, state: WEnum::Value(wl_keyboard::KeyState::Pressed), .. } => {
                state.input_serial = serial;
                let Some(table) = &state.lookup else { return };
                let kc = Keycode::from_evdev(key);
                let (dep, lat, lck, group) = state.mods;
                let mods = ModifierMask(dep | lat | lck);
                let base = table.lookup(GroupIndex(0), ModifierMask(0), kc).into_iter().next().map(|l| l.keysym().0);
                let ctrl = (dep | lat) & 4 != 0;
                let shift = (dep | lat) & 1 != 0;
                let k = base.and_then(keys::key_from_sym);
                if std::env::var_os("ZYRIK_DEBUG_INPUT").is_some() {
                    eprintln!("zyrik: клавиша {key}: base {base:x?}, mods {dep:x}/{lat:x}/{lck:x} группа {group} → {k:?}, ctrl {ctrl}");
                }
                let text: Vec<char> = table.lookup(GroupIndex(group), mods, kc).into_iter().filter_map(|l| l.char()).collect();
                if let Some(k) = k {
                    state.input(Input::Key { key: k, shift, ctrl });
                }
                let control = matches!(k, Some(Key::Enter | Key::Backspace | Key::Esc));
                if !ctrl && !control {
                    for c in text.into_iter().filter(|c| !c.is_control()) {
                        state.input(Input::Char(c));
                    }
                }
            }
            _ => {}
        }
    }
}

/// Раскладка от композитора (текст XKB в памяти) → таблица «клавиша → символ».
fn load_keymap(fd: &OwnedFd, size: usize) -> Option<LookupTable> {
    let map = unsafe { libc::mmap(std::ptr::null_mut(), size, libc::PROT_READ, libc::MAP_PRIVATE, fd.as_raw_fd(), 0) };
    if map == libc::MAP_FAILED {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(map.cast::<u8>(), size) };
    let text = bytes.split(|b| *b == 0).next().unwrap_or(&[]).to_vec();
    unsafe {
        libc::munmap(map, size);
    }
    let ctx = kbvm::xkb::Context::default();
    let keymap = ctx.keymap_from_bytes(WriteToLog, None, &text).ok()?;
    Some(keymap.to_builder().build_lookup_table())
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for State {
    fn event(_: &mut Self, _: &ZwlrDataControlDeviceV1, event: zwlr_data_control_device_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        // чужое содержимое буфера не читаем: предложения сразу закрываем
        match event {
            zwlr_data_control_device_v1::Event::DataOffer { id } => id.destroy(),
            zwlr_data_control_device_v1::Event::Selection { id: Some(o) } | zwlr_data_control_device_v1::Event::PrimarySelection { id: Some(o) } => o.destroy(),
            _ => {}
        }
    }

    event_created_child!(State, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl State {
    /// Кто-то просит содержимое буфера. Wayland не говорит, кто и зажат ли Ctrl. Менеджеры
    /// буфера реагируют на смену буфера мгновенно — им не отдаём; вставка человеком — позже.
    fn send_clip(&mut self, mime: &str, fd: OwnedFd) {
        let c = &mut self.clip;
        let mut f = std::fs::File::from(fd);
        if mime == "x-kde-passwordManagerHint" {
            let _ = f.write_all(b"secret");
            return;
        }
        if mime != "image/png" || c.image.is_none() {
            return;
        }
        if c.set_at.elapsed() < WATCHER_WINDOW {
            eprintln!("zyrik: буфер: запрос сразу после копирования — менеджеру буфера не отдаём");
            return;
        }
        if c.png.is_none() {
            c.png = c.image.as_ref().map(encode_png);
        }
        if let Some(png) = &c.png {
            let _ = f.write_all(png);
        }
        c.clear_at = Some(Instant::now() + CLEAR_AFTER);
    }

    /// Кто-то положил в буфер своё — наш снимок больше не нужен.
    fn clip_cancelled(&mut self, src: Source) {
        src.destroy();
        if self.clip.source.as_ref() == Some(&src) {
            self.clip.source = None;
            wipe(&mut self.clip);
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for State {
    fn event(state: &mut Self, src: &ZwlrDataControlSourceV1, event: zwlr_data_control_source_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            zwlr_data_control_source_v1::Event::Send { mime_type, fd } => state.send_clip(&mime_type, fd),
            zwlr_data_control_source_v1::Event::Cancelled => state.clip_cancelled(Source::Wlr(src.clone())),
            _ => {}
        }
    }
}

impl Dispatch<ExtDataControlSourceV1, ()> for State {
    fn event(state: &mut Self, src: &ExtDataControlSourceV1, event: ext_data_control_source_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => state.send_clip(&mime_type, fd),
            ext_data_control_source_v1::Event::Cancelled => state.clip_cancelled(Source::Ext(src.clone())),
            _ => {}
        }
    }
}

impl Dispatch<WlDataSource, ()> for State {
    fn event(state: &mut Self, src: &WlDataSource, event: wl_data_source::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_data_source::Event::Send { mime_type, fd } => state.send_clip(&mime_type, fd),
            wl_data_source::Event::Cancelled => state.clip_cancelled(Source::Core(src.clone())),
            _ => {}
        }
    }
}

impl Dispatch<ExtDataControlDeviceV1, ()> for State {
    fn event(_: &mut Self, _: &ExtDataControlDeviceV1, event: ext_data_control_device_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            ext_data_control_device_v1::Event::DataOffer { id } => id.destroy(),
            ext_data_control_device_v1::Event::Selection { id: Some(o) } | ext_data_control_device_v1::Event::PrimarySelection { id: Some(o) } => o.destroy(),
            _ => {}
        }
    }

    event_created_child!(State, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ()),
    ]);
}

impl Dispatch<WlDataDevice, ()> for State {
    fn event(_: &mut Self, _: &WlDataDevice, event: wl_data_device::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            wl_data_device::Event::DataOffer { id } => id.destroy(),
            wl_data_device::Event::Selection { id: Some(o) } => o.destroy(),
            _ => {}
        }
    }

    event_created_child!(State, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
}
