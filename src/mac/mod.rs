//! Прослойка macOS (фаза 4). Ядро то же; здесь AppKit, ScreenCaptureKit и Carbon (горячие клавиши).
//!
//! Всё работает в главном потоке: `next_command` сам крутит очередь событий AppKit
//! (`nextEventMatchingMask…`), события окон складываются в очередь команд.
//!
//! Координаты. Ядру нужен один кадр на все мониторы в пикселях. Мониторы macOS живут в точках,
//! у каждого свой масштаб (Retina — 2). Берём наибольший масштаб `S` и раскладываем мониторы
//! в пикселях: (точки − левый верхний угол всех экранов) × S. На каждый монитор — своё окно
//! выделения (одно окно на несколько экранов macOS не растягивает); каждое показывает свой
//! кусок общего кадра.
//!
//! Принцип I: ничего не пишем на диск, не ходим в сеть. Буфер — с пометками
//! `org.nspasteboard.TransientType` и `ConcealedType` (менеджеры буфера их не сохраняют).

mod keys;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAlert, NSAlertStyle, NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSColor, NSCursor,
    NSEvent, NSEventMask, NSEventModifierFlags, NSGraphicsContext, NSImage, NSMenu, NSMenuItem, NSPasteboard,
    NSPasteboardItem, NSPasteboardItemDataProvider, NSPasteboardTypePNG, NSRunningApplication, NSScreen,
    NSScreenSaverWindowLevel, NSStatusBar, NSStatusItem, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize, CFRetained};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGImage,
    CGEventFlags, CGEventSource, CGEventSourceStateID, CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess,
};
use objc2_foundation::{
    NSArray, NSBundle, NSData, NSDate, NSDefaultRunLoopMode, NSError, NSLocale, NSNumber, NSObject,
    NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCScreenshotManager, SCShareableContent, SCStreamConfiguration, SCWindow,
};

use zyrik::export::{encode_png, Stamp};
use zyrik::hotkey::Hotkey;
use zyrik::image::Image;
use zyrik::oneshot::{Answer, OneShot};
use zyrik::platform::{Command, Desktop, Frame, MenuItem, Monitor, Place, PlaceKind, Platform, Problem, Shot};
use zyrik::session::{Button, Cursor, Input, Pt, Rect};

/// kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little: в памяти B,G,R,A — как наш u32.
const BGRA: u32 = 2 | (2 << 12);
const WATCH: Duration = Duration::from_millis(30);

/// Монитор: где он в точках AppKit и где его кусок в общем кадре.
#[derive(Clone, Copy)]
struct Screen {
    display: u32,
    /// В точках, начало — левый нижний угол основного экрана (как у AppKit)
    frame: NSRect,
    /// Кусок общего кадра, пиксели
    px: Rect,
}

#[derive(Default)]
struct Shared {
    queue: VecDeque<Command>,
    menu: Vec<MenuItem>,
    /// `hotkeys[id - 1]`
    hotkeys: Vec<Shot>,
    /// Наибольший масштаб экранов — пикселей в точке
    scale: f64,
    screens: Vec<Screen>,
    /// Картинки для окон выделения, по монитору
    images: Vec<Option<CFRetained<CGImage>>>,
    shift: bool,
    clip: Clip,
}

#[derive(Default)]
struct Clip {
    image: Option<Image>,
    oneshot: OneShot,
    watching: bool,
    /// changeCount буфера, когда в нём наш снимок
    count: isize,
    /// После отказа нужно заново положить обещание
    rearm: bool,
    rearms: u32,
    /// Пишем новое обещание: сообщения о старом не должны затереть снимок
    writing: bool,
}

thread_local! {
    static SHARED: RefCell<Shared> = RefCell::new(Shared::default());
}

fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    SHARED.with(|s| f(&mut s.borrow_mut()))
}

fn push(c: Command) {
    with(|s| s.queue.push_back(c));
}

fn input(i: Input) {
    push(Command::Overlay(i));
}

// ---------- классы Objective-C ----------

pub struct ViewIvars {
    screen: usize,
}

define_class!(
    /// Содержимое окна выделения: рисует свой кусок кадра и ловит мышь и клавиши.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZyrikView"]
    #[ivars = ViewIvars]
    struct ZView;

    impl ZView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _e: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _r: NSRect) {
            let i = self.ivars().screen;
            let img = with(|s| s.images.get(i).cloned().flatten());
            let (Some(img), Some(ctx)) = (img, NSGraphicsContext::currentContext()) else { return };
            CGContext::draw_image(Some(&ctx.CGContext()), self.bounds(), Some(&img));
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, e: &NSEvent) {
            let p = self.point(e);
            input(if e.clickCount() == 2 { Input::DoubleClick(p) } else { Input::Down(Button::Left, p) });
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, e: &NSEvent) {
            input(Input::Up(Button::Left, self.point(e)));
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, e: &NSEvent) {
            input(Input::Move(self.point(e)));
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, e: &NSEvent) {
            input(Input::Move(self.point(e)));
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_down(&self, e: &NSEvent) {
            input(Input::Down(Button::Right, self.point(e)));
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_up(&self, e: &NSEvent) {
            input(Input::Up(Button::Right, self.point(e)));
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll(&self, e: &NSEvent) {
            let dy = e.scrollingDeltaY();
            // тачпад шлёт много мелких шагов — собираем в «щелчки»
            let notches = if e.hasPreciseScrollingDeltas() { (dy / 10.0).round() } else { dy.signum() };
            if notches != 0.0 {
                input(Input::Wheel(notches as i32, self.point(e)));
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, e: &NSEvent) {
            let flags = e.modifierFlags();
            // Cmd на Mac — то же, что Ctrl в Windows (Cmd+C, Cmd+S, Cmd+Z)
            let ctrl = flags.contains(NSEventModifierFlags::Command) || flags.contains(NSEventModifierFlags::Control);
            let shift = flags.contains(NSEventModifierFlags::Shift);
            let key = keys::key(e.keyCode());
            if let Some(k) = key {
                input(Input::Key { key: k, shift, ctrl });
            }
            let control = matches!(key, Some(zyrik::session::Key::Enter | zyrik::session::Key::Backspace | zyrik::session::Key::Esc));
            if !ctrl && !control {
                if let Some(chars) = e.characters() {
                    for c in chars.to_string().chars().filter(|c| !c.is_control() && !('\u{F700}'..='\u{F8FF}').contains(c)) {
                        input(Input::Char(c));
                    }
                }
            }
        }

        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, e: &NSEvent) {
            let shift = e.modifierFlags().contains(NSEventModifierFlags::Shift);
            if with(|s| std::mem::replace(&mut s.shift, shift)) != shift {
                input(Input::Shift(shift));
            }
        }
    }
);

impl ZView {
    fn new(mtm: MainThreadMarker, frame: NSRect, screen: usize) -> Retained<ZView> {
        let this = ZView::alloc(mtm).set_ivars(ViewIvars { screen });
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// Точка события → пиксели общего кадра.
    fn point(&self, e: &NSEvent) -> Pt {
        let loc = e.locationInWindow();
        let (scale, scr) = with(|s| (s.scale, s.screens.get(self.ivars().screen).copied()));
        let Some(scr) = scr else { return Pt { x: 0, y: 0 } };
        Pt {
            x: scr.px.x + (loc.x * scale).round() as i32,
            y: scr.px.y + ((scr.frame.size.height - loc.y) * scale).round() as i32,
        }
    }
}

define_class!(
    /// Окно без рамки, которое всё же может принимать клавиатуру.
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZyrikWindow"]
    struct ZWindow;

    impl ZWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            true
        }
    }
);

define_class!(
    /// Пункты меню значка и отложенная выдача снимка в буфер.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZyrikTarget"]
    struct ZTarget;

    impl ZTarget {
        #[unsafe(method(menuItem:))]
        fn menu_item(&self, item: &NSMenuItem) {
            let i = item.tag() as usize;
            if let Some(c) = with(|s| s.menu.get(i).map(|m| m.command.clone())) {
                push(c);
            }
        }
    }

    unsafe impl NSObjectProtocol for ZTarget {}

    unsafe impl NSPasteboardItemDataProvider for ZTarget {
        #[unsafe(method(pasteboard:item:provideDataForType:))]
        fn provide(&self, _pb: Option<&NSPasteboard>, item: &NSPasteboardItem, ty: &NSString) {
            provide(item, ty);
        }

        #[unsafe(method(pasteboardFinishedWithDataProvider:))]
        fn finished(&self, pb: &NSPasteboard) {
            // наше же новое обещание (после отказа) — не повод затирать снимок
            if !with(|s| s.clip.writing) && pb.changeCount() != with(|s| s.clip.count) {
                wipe_clip();
            }
        }
    }
);

impl ZTarget {
    fn new(mtm: MainThreadMarker) -> Retained<ZTarget> {
        let this = ZTarget::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

// ---------- Carbon: глобальные горячие клавиши ----------

#[repr(C)]
#[derive(Clone, Copy)]
struct EventHotKeyID {
    signature: u32,
    id: u32,
}

#[repr(C)]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn RegisterEventHotKey(code: u32, mods: u32, id: EventHotKeyID, target: *mut c_void, options: u32, out: *mut *mut c_void) -> i32;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32,
        count: usize,
        list: *const EventTypeSpec,
        user: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32;
    fn GetEventParameter(
        event: *mut c_void,
        name: u32,
        desired: u32,
        actual: *mut u32,
        size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> i32;
}

const fn four(s: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*s)
}

/// eventHotKeyExistsErr
const HOTKEY_EXISTS: i32 = -9878;

extern "C" fn on_hotkey(_next: *mut c_void, event: *mut c_void, _user: *mut c_void) -> i32 {
    let mut id = EventHotKeyID { signature: 0, id: 0 };
    let r = unsafe {
        GetEventParameter(
            event,
            four(b"----"),
            four(b"hkid"),
            std::ptr::null_mut(),
            std::mem::size_of::<EventHotKeyID>(),
            std::ptr::null_mut(),
            (&mut id as *mut EventHotKeyID).cast(),
        )
    };
    if r == 0 {
        if let Some(shot) = with(|s| s.hotkeys.get((id.id as usize).wrapping_sub(1)).copied()) {
            push(Command::Capture(shot));
        }
    }
    0
}

// ---------- буфер обмена ----------

fn wipe_clip() {
    with(|s| {
        if let Some(mut img) = s.clip.image.take() {
            img.wipe();
        }
        s.clip.watching = false;
    });
}

/// Cmd или Ctrl зажат — по общему состоянию клавиатуры сеанса (`NSEvent.modifierFlags`
/// у программы без активного окна отпускание не замечал).
fn modifier_held() -> bool {
    let f = CGEventSource::flags_state(CGEventSourceStateID::CombinedSessionState);
    f.contains(CGEventFlags::MaskCommand) || f.contains(CGEventFlags::MaskControl)
}

/// Сколько раз на один снимок заново кладём обещание после отказа фоновому читателю
const MAX_REARMS: u32 = 20;

/// Положить в буфер обещание PNG (данные — по запросу, см. `provide`) с пометками
/// nspasteboard.org «не в историю» и «секретно». Возвращает changeCount буфера.
fn promise(target: &ZTarget) -> Result<isize, Problem> {
    with(|s| s.clip.writing = true);
    let r = write_promise(target);
    with(|s| s.clip.writing = false);
    r
}

fn write_promise(target: &ZTarget) -> Result<isize, Problem> {
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let item = NSPasteboardItem::new();
    let png = unsafe { NSPasteboardTypePNG };
    let types = NSArray::from_slice(&[png]);
    if !item.setDataProvider_forTypes(ProtocolObject::from_ref(target), &types) {
        return Err(Problem::Os("NSPasteboardItem: setDataProvider".into()));
    }
    let empty = NSData::new();
    item.setData_forType(&empty, &ns("org.nspasteboard.TransientType"));
    item.setData_forType(&empty, &ns("org.nspasteboard.ConcealedType"));
    let objs = NSArray::from_retained_slice(&[ProtocolObject::from_retained(item)]);
    if !pb.writeObjects(&objs) {
        return Err(Problem::Os("NSPasteboard: writeObjects".into()));
    }
    Ok(pb.changeCount())
}

/// Программа просит PNG. Что она «на переднем плане», macOS не сообщает — отдаём только
/// с зажатым Cmd/Ctrl (Cmd+V), как фоновым читателям в Windows не отдаём.
fn provide(item: &NSPasteboardItem, ty: &NSString) {
    if ty != unsafe { NSPasteboardTypePNG } {
        eprintln!("zyrik: буфер: просят {ty} — не отдаём");
        return;
    }
    let held = modifier_held();
    let png = with(|s| {
        s.clip.image.as_ref()?;
        let answer = s.clip.oneshot.requested(false, held);
        eprintln!("zyrik: буфер: просят PNG, Cmd/Ctrl {}, ответ {answer:?}", if held { "зажат" } else { "не зажат" });
        match answer {
            Answer::Refuse => {
                // macOS после отказа больше не спрашивает у нас данные этого обещания —
                // кладём новое, иначе настоящая вставка получила бы пустоту
                if s.clip.rearms < MAX_REARMS {
                    s.clip.rearm = true;
                }
                None
            }
            a => {
                if a == Answer::GiveAndWatch {
                    s.clip.watching = true;
                }
                s.clip.image.as_ref().map(encode_png)
            }
        }
    });
    if let Some(png) = png {
        let data = NSData::with_bytes(&png);
        item.setData_forType(&data, ty);
    }
}

/// `--clear-clipboard`: время вышло. Содержимое не читаем: наш ли снимок — по changeCount.
fn clear_clip_if_ours() {
    let count = with(|s| s.clip.count);
    let pb = NSPasteboard::generalPasteboard();
    if count != 0 && pb.changeCount() == count {
        pb.clearContents();
    }
    wipe_clip();
}

/// Окна сверху вниз, активное окно и курсор в пикселях кадра (история 5).
/// `CGWindowListCopyWindowInfo` отдаёт окна спереди назад; границы — в точках, начало
/// в левом верхнем углу главного экрана. Имена окон не берём — только границы и владельца.
fn desktop(scale: f64, screens: &[Screen]) -> Desktop {
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSWorkspace;
    use objc2_core_graphics::{CGWindowListCopyWindowInfo, CGWindowListOption};
    use objc2_foundation::NSDictionary;

    let Some(main) = screens.first() else { return Desktop::default() };
    let min_x = screens.iter().map(|s| s.frame.origin.x).fold(f64::MAX, f64::min);
    let max_y = screens.iter().map(|s| s.frame.origin.y + s.frame.size.height).fold(f64::MIN, f64::max);
    let main_h = main.frame.size.height;
    let px = |v: f64| (v * scale).round() as i32;

    let num = |d: &NSDictionary<NSString, AnyObject>, k: &str| -> Option<f64> {
        d.objectForKey(&ns(k)).and_then(|o| o.downcast::<NSNumber>().ok()).map(|n| n.doubleValue())
    };
    let me = std::process::id() as f64;
    let mut windows = Vec::new();
    let mut owners = Vec::new();
    let opts = CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    if let Some(list) = CGWindowListCopyWindowInfo(opts, 0) {
        // CFArray и NSArray — один и тот же объект (toll-free bridging)
        let list: &NSArray<NSDictionary<NSString, AnyObject>> =
            unsafe { &*(CFRetained::as_ptr(&list).as_ptr() as *const NSArray<NSDictionary<NSString, AnyObject>>) };
        for d in list.iter() {
            // слой 0 — обычные окна; меню, Dock и панели выше
            if num(&d, "kCGWindowLayer") != Some(0.0) || num(&d, "kCGWindowAlpha") == Some(0.0) {
                continue;
            }
            let pid = num(&d, "kCGWindowOwnerPID").unwrap_or(-1.0);
            if pid == me {
                continue;
            }
            let Some(b) = d.objectForKey(&ns("kCGWindowBounds")) else { continue };
            let b: &NSDictionary<NSString, AnyObject> =
                unsafe { &*(Retained::as_ptr(&b) as *const NSDictionary<NSString, AnyObject>) };
            let (Some(x), Some(y), Some(w), Some(h)) = (num(b, "X"), num(b, "Y"), num(b, "Width"), num(b, "Height"))
            else {
                continue;
            };
            let r = Rect { x: px(x - min_x), y: px(y + max_y - main_h), w: px(w), h: px(h) };
            if r.w > 0 && r.h > 0 {
                windows.push(r);
                owners.push(pid);
            }
        }
    }
    let front = NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier() as f64);
    let active = front.and_then(|pid| owners.iter().position(|&o| o == pid)).map(|i| windows[i]);
    let m = NSEvent::mouseLocation();
    let cursor = Some(Pt { x: px(m.x - min_x), y: px(max_y - m.y) });
    Desktop { windows, active, cursor }
}

/// Cmd отпустили после вставки — очищаем буфер, если там всё ещё наш снимок.
fn watch_clip() {
    let (watching, count) = with(|s| (s.clip.watching, s.clip.count));
    if !watching || modifier_held() {
        return;
    }
    let pb = NSPasteboard::generalPasteboard();
    let ours = pb.changeCount() == count;
    if ours {
        pb.clearContents();
    }
    eprintln!("zyrik: буфер: Cmd отпущен — {}", if ours { "очищен" } else { "там уже не наш снимок" });
    wipe_clip();
}

// ---------- платформа ----------

pub struct Mac {
    mtm: MainThreadMarker,
    app: Retained<NSApplication>,
    target: Retained<ZTarget>,
    status: Option<Retained<NSStatusItem>>,
    windows: Vec<Retained<ZWindow>>,
    timers: Vec<(Instant, Command)>,
    cursor: Option<Cursor>,
}

fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

static ICO: &[u8] = include_bytes!("../../assets/zyrik.ico");

/// PNG 256×256 из .ico — для значка в строке меню.
fn icon_png() -> Option<&'static [u8]> {
    let rd = |i: usize, n: usize| -> usize { ICO[i..i + n].iter().rev().fold(0, |a, b| a << 8 | *b as usize) };
    (0..rd(4, 2)).find_map(|e| {
        let d = 6 + 16 * e;
        let (size, off) = (rd(d + 8, 4), rd(d + 12, 4));
        let b = ICO.get(off..off + size)?;
        b.starts_with(b"\x89PNG").then_some(b)
    })
}

impl Mac {
    pub fn new() -> Result<Mac, String> {
        let mtm = MainThreadMarker::new().ok_or("нужен главный поток")?;
        let app = NSApplication::sharedApplication(mtm);
        // без значка в Dock: Зырик живёт в строке меню
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let target = ZTarget::new(mtm);
        let spec = EventTypeSpec { class: four(b"keyb"), kind: 5 };
        let r = unsafe {
            InstallEventHandler(GetApplicationEventTarget(), on_hotkey, 1, &spec, std::ptr::null_mut(), std::ptr::null_mut())
        };
        if r != 0 {
            return Err(format!("InstallEventHandler: {r}"));
        }
        Ok(Mac { mtm, app, target, status: None, windows: Vec::new(), timers: Vec::new(), cursor: None })
    }

    /// Мониторы сейчас: масштаб и куски общего кадра.
    fn layout(&self) -> (f64, Vec<Screen>, u32, u32) {
        let screens = NSScreen::screens(self.mtm);
        let list: Vec<(u32, NSRect, f64)> = screens
            .iter()
            .map(|s| {
                let id = s
                    .deviceDescription()
                    .objectForKey(&ns("NSScreenNumber"))
                    .and_then(|o| o.downcast::<NSNumber>().ok())
                    .map_or(0, |n| n.unsignedIntValue());
                (id, s.frame(), s.backingScaleFactor())
            })
            .collect();
        let scale = list.iter().map(|l| l.2).fold(1.0, f64::max);
        let min_x = list.iter().map(|l| l.1.origin.x).fold(f64::MAX, f64::min);
        let max_y = list.iter().map(|l| l.1.origin.y + l.1.size.height).fold(f64::MIN, f64::max);
        let r = |v: f64| (v * scale).round() as i32;
        let out: Vec<Screen> = list
            .iter()
            .map(|&(display, frame, _)| Screen {
                display,
                frame,
                px: Rect {
                    x: r(frame.origin.x - min_x),
                    y: r(max_y - frame.origin.y - frame.size.height),
                    w: r(frame.size.width),
                    h: r(frame.size.height),
                },
            })
            .collect();
        let w = out.iter().map(|s| s.px.right()).max().unwrap_or(1).max(1) as u32;
        let h = out.iter().map(|s| s.px.bottom()).max().unwrap_or(1).max(1) as u32;
        (scale, out, w, h)
    }

    fn pump(&self, until: Option<Instant>) {
        let wait = until.map(|t| t.saturating_duration_since(Instant::now()).as_secs_f64());
        let date = match wait {
            Some(s) => NSDate::dateWithTimeIntervalSinceNow(s),
            None => NSDate::distantFuture(),
        };
        let mode = unsafe { NSDefaultRunLoopMode };
        if let Some(e) = self.app.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, Some(&date), mode, true) {
            self.app.sendEvent(&e);
        }
    }
}

/// Снять один монитор ScreenCaptureKit'ом (macOS 14+) в кусок кадра `px` (BGRA, пиксели).
fn capture_display(display: &SCDisplay, px: Rect) -> Result<Vec<u32>, Problem> {
    let filter = unsafe { SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), display, &NSArray::<SCWindow>::new()) };
    let cfg = unsafe { SCStreamConfiguration::new() };
    unsafe {
        cfg.setWidth(px.w as usize);
        cfg.setHeight(px.h as usize);
        cfg.setShowsCursor(false);
    }
    let (tx, rx) = mpsc::channel::<Option<CFRetained<CGImage>>>();
    let block = RcBlock::new(move |img: *mut CGImage, _err: *mut NSError| {
        let img = NonNull::new(img).map(|p| unsafe { CFRetained::retain(p) });
        let _ = tx.send(img);
    });
    unsafe { SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(&filter, &cfg, Some(&block)) };
    let img = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| Problem::Os("ScreenCaptureKit не ответил".into()))?
        .ok_or(Problem::NoPermission)?;
    let mut buf = vec![0u32; (px.w * px.h) as usize];
    let space = CGColorSpace::new_device_rgb();
    let ctx = unsafe {
        CGBitmapContextCreate(buf.as_mut_ptr().cast(), px.w as usize, px.h as usize, 8, px.w as usize * 4, space.as_deref(), BGRA)
    }
    .ok_or_else(|| Problem::Os("CGBitmapContextCreate".into()))?;
    let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(f64::from(px.w), f64::from(px.h)));
    CGContext::draw_image(Some(&ctx), rect, Some(&img));
    drop(ctx);
    for p in &mut buf {
        *p |= 0xFF00_0000;
    }
    Ok(buf)
}

fn shareable_displays() -> Result<Retained<NSArray<SCDisplay>>, Problem> {
    let (tx, rx) = mpsc::channel::<Option<Retained<NSArray<SCDisplay>>>>();
    let block = RcBlock::new(move |content: *mut SCShareableContent, _err: *mut NSError| {
        let list = unsafe { content.as_ref() }.map(|c| unsafe { c.displays() });
        let _ = tx.send(list);
    });
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| Problem::Os("ScreenCaptureKit не ответил".into()))?
        .ok_or(Problem::NoPermission)
}

impl Platform for Mac {
    fn system_lang(&self) -> Option<String> {
        NSLocale::preferredLanguages().firstObject().map(|s| s.to_string())
    }

    fn claim_single_instance(&mut self) -> Result<(), Problem> {
        // по идентификатору пакета Зырик.app; голый бинарник (разработка) не проверяем
        let Some(id) = NSBundle::mainBundle().bundleIdentifier() else { return Ok(()) };
        let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(&id);
        if running.count() > 1 {
            return Err(Problem::AlreadyRunning);
        }
        Ok(())
    }

    fn show_tray(&mut self, tip: &str, menu: &[MenuItem]) -> Result<(), Problem> {
        let mtm = self.mtm;
        with(|s| s.menu = menu.to_vec());
        let item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
        if let Some(button) = item.button(mtm) {
            match icon_png().and_then(|png| NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png))) {
                Some(img) => {
                    img.setSize(NSSize::new(18.0, 18.0));
                    button.setImage(Some(&img));
                }
                None => button.setTitle(&ns("о_О")),
            }
            button.setToolTip(Some(&ns(tip)));
        }
        let m = NSMenu::new(mtm);
        for (i, entry) in menu.iter().enumerate() {
            let it = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &ns(&entry.text), Some(sel!(menuItem:)), &ns(""))
            };
            unsafe { it.setTarget(Some(&self.target)) };
            it.setTag(i as isize);
            m.addItem(&it);
        }
        item.setMenu(Some(&m));
        self.status = Some(item);
        Ok(())
    }

    fn set_tray_tip(&mut self, tip: &str) {
        if let Some(b) = self.status.as_ref().and_then(|s| s.button(self.mtm)) {
            b.setToolTip(Some(&ns(tip)));
        }
    }

    fn register_hotkey(&mut self, shot: Shot, key: Hotkey) -> Result<(), Problem> {
        let code = keys::hotkey_code(key.key).ok_or_else(|| Problem::Os(format!("{key}: нет такой клавиши на Mac")))?;
        let m = key.mods;
        let mut mods = 0u32;
        for (on, bit) in [(m.meta, 1 << 8), (m.shift, 1 << 9), (m.alt, 1 << 11), (m.ctrl, 1 << 12)] {
            if on {
                mods |= bit;
            }
        }
        let id = with(|s| {
            s.hotkeys.push(shot);
            s.hotkeys.len() as u32
        });
        let hk = EventHotKeyID { signature: four(b"Zyrk"), id };
        let mut out = std::ptr::null_mut();
        let r = unsafe { RegisterEventHotKey(u32::from(code), mods, hk, GetApplicationEventTarget(), 0, &mut out) };
        match r {
            0 => Ok(()),
            HOTKEY_EXISTS => Err(Problem::KeyBusy),
            e => Err(Problem::Os(format!("RegisterEventHotKey: {e}"))),
        }
    }

    fn schedule(&mut self, command: Command, delay_ms: u32) {
        self.timers.push((Instant::now() + Duration::from_millis(u64::from(delay_ms)), command));
    }

    fn next_command(&mut self) -> Option<Command> {
        loop {
            if with(|s| std::mem::take(&mut s.clip.rearm)) {
                match promise(&self.target) {
                    Ok(count) => with(|s| {
                        s.clip.count = count;
                        s.clip.rearms += 1;
                    }),
                    Err(e) => eprintln!("zyrik: буфер: {e:?}"),
                }
            }
            watch_clip();
            if let Some(c) = with(|s| s.queue.pop_front()) {
                return Some(c);
            }
            let now = Instant::now();
            if let Some(i) = self.timers.iter().position(|(t, _)| *t <= now) {
                return Some(self.timers.remove(i).1);
            }
            let mut until = self.timers.iter().map(|(t, _)| *t).min();
            if with(|s| s.clip.watching) {
                let soon = now + WATCH;
                until = Some(until.map_or(soon, |t| t.min(soon)));
            }
            self.pump(until);
        }
    }

    fn capture(&mut self) -> Result<Frame, Problem> {
        if !CGPreflightScreenCaptureAccess() {
            // первый раз macOS покажет свой запрос; разрешение действует после перезапуска
            CGRequestScreenCaptureAccess();
            return Err(Problem::NoPermission);
        }
        let (scale, screens, w, h) = self.layout();
        let displays = shareable_displays()?;
        let mut px = vec![0xFF00_0000u32; (w * h) as usize];
        let mut monitors = Vec::new();
        for scr in &screens {
            let Some(d) = displays.iter().find(|d| unsafe { d.displayID() } == scr.display) else { continue };
            let mut part = capture_display(&d, scr.px)?;
            for y in 0..scr.px.h {
                let src = &part[(y * scr.px.w) as usize..((y + 1) * scr.px.w) as usize];
                let at = ((scr.px.y + y) as u32 * w + scr.px.x as u32) as usize;
                px[at..at + scr.px.w as usize].copy_from_slice(src);
            }
            zeroize::Zeroize::zeroize(&mut part);
            monitors.push(Monitor { rect: scr.px, scale: (scale * 100.0).round() as u32 });
        }
        let desktop = desktop(scale, &screens);
        with(|s| {
            s.scale = scale;
            s.screens = screens;
        });
        Ok(Frame { x: 0, y: 0, image: Image::from_pixels(w, h, px), monitors, desktop })
    }

    fn show_overlay(&mut self, _x: i32, _y: i32, _w: u32, _h: u32) -> Result<(), Problem> {
        let mtm = self.mtm;
        let screens = with(|s| s.screens.clone());
        self.windows.clear();
        for (i, scr) in screens.iter().enumerate() {
            let win: Retained<ZWindow> = unsafe {
                msg_send![ZWindow::alloc(mtm), initWithContentRect: scr.frame, styleMask: NSWindowStyleMask::Borderless, backing: NSBackingStoreType::Buffered, defer: false]
            };
            unsafe { win.setReleasedWhenClosed(false) };
            win.setLevel(NSScreenSaverWindowLevel);
            win.setOpaque(true);
            win.setHasShadow(false);
            win.setBackgroundColor(Some(&NSColor::blackColor()));
            win.setAcceptsMouseMovedEvents(true);
            let bounds = NSRect::new(NSPoint::new(0.0, 0.0), scr.frame.size);
            let view = ZView::new(mtm, bounds, i);
            win.setContentView(Some(&view));
            win.setFrame_display(scr.frame, false);
            win.orderFrontRegardless();
            self.windows.push(win);
        }
        #[allow(deprecated)]
        self.app.activateIgnoringOtherApps(true);
        // клавиатура — окну на мониторе с курсором
        let mouse = NSEvent::mouseLocation();
        let key = screens.iter().position(|s| {
            let f = s.frame;
            mouse.x >= f.origin.x && mouse.x < f.origin.x + f.size.width && mouse.y >= f.origin.y && mouse.y < f.origin.y + f.size.height
        });
        if let Some(w) = self.windows.get(key.unwrap_or(0)) {
            w.makeKeyWindow();
            w.makeFirstResponder(w.contentView().as_deref().map(|v| &**v));
        }
        Ok(())
    }

    fn present(&mut self, image: &Image) {
        let screens = with(|s| s.screens.clone());
        let space = CGColorSpace::new_device_rgb();
        let w = image.width() as usize;
        let mut images = Vec::new();
        for scr in &screens {
            let (pw, ph) = (scr.px.w as usize, scr.px.h as usize);
            let mut part = Vec::with_capacity(pw * ph);
            for y in 0..ph {
                let at = (scr.px.y as usize + y) * w + scr.px.x as usize;
                part.extend_from_slice(&image.pixels()[at..at + pw]);
            }
            let ctx = unsafe { CGBitmapContextCreate(part.as_mut_ptr().cast(), pw, ph, 8, pw * 4, space.as_deref(), BGRA) };
            images.push(ctx.and_then(|c| CGBitmapContextCreateImage(Some(&c))));
            zeroize::Zeroize::zeroize(&mut part);
        }
        with(|s| s.images = images);
        for win in &self.windows {
            if let Some(v) = win.contentView() {
                v.setNeedsDisplay(true);
            }
            win.displayIfNeeded();
        }
    }

    fn set_cursor(&mut self, cursor: Cursor) {
        if self.cursor == Some(cursor) {
            return;
        }
        self.cursor = Some(cursor);
        // старые стрелки ↔ ↕: новые API — только с macOS 15
        #[allow(deprecated)]
        let c = match cursor {
            Cursor::Arrow => NSCursor::arrowCursor(),
            Cursor::Text => NSCursor::IBeamCursor(),
            Cursor::Move => NSCursor::openHandCursor(),
            Cursor::ResizeWe => NSCursor::resizeLeftRightCursor(),
            Cursor::ResizeNs => NSCursor::resizeUpDownCursor(),
            // диагональных стрелок в публичном API до macOS 15 нет
            Cursor::Cross | Cursor::ResizeNwse | Cursor::ResizeNesw => NSCursor::crosshairCursor(),
        };
        c.set();
    }

    fn hide_overlay(&mut self) {
        with(|s| s.images.clear());
        for w in self.windows.drain(..) {
            w.orderOut(None);
        }
        self.cursor = None;
        NSCursor::arrowCursor().set();
    }

    fn set_clipboard_image(&mut self, image: Image) -> Result<(), Problem> {
        wipe_clip();
        // сначала снимок, потом обещание; пока пишем, «finished» старого обещания не затрёт новое
        with(|s| {
            s.clip.image = Some(image);
            s.clip.oneshot = OneShot::new();
            s.clip.rearm = false;
            s.clip.rearms = 0;
        });
        match promise(&self.target) {
            Ok(count) => {
                with(|s| s.clip.count = count);
                Ok(())
            }
            Err(e) => {
                wipe_clip();
                Err(e)
            }
        }
    }

    fn clear_clipboard_if_ours(&mut self) {
        clear_clip_if_ours();
    }

    fn places(&self) -> Vec<Place> {
        let mut out = Vec::new();
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            for (kind, name) in [
                (PlaceKind::Pictures, "Pictures"),
                (PlaceKind::Desktop, "Desktop"),
                (PlaceKind::Documents, "Documents"),
                (PlaceKind::Downloads, "Downloads"),
            ] {
                let p = home.join(name);
                if p.is_dir() {
                    out.push(Place { kind, path: p });
                }
            }
            out.push(Place { kind: PlaceKind::Home, path: home });
        }
        out.push(Place { kind: PlaceKind::Drive, path: PathBuf::from("/") });
        if let Ok(names) = zyrik::picker::list_dirs(std::path::Path::new("/Volumes")) {
            out.extend(names.into_iter().map(|n| Place { kind: PlaceKind::Drive, path: PathBuf::from("/Volumes").join(n) }));
        }
        out
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
        message(self.mtm, title, text, error);
    }

    fn debug_log(&self, text: &str) {
        eprintln!("zyrik: {text}");
    }
}

pub fn message(mtm: MainThreadMarker, title: &str, text: &str, error: bool) {
    eprintln!("{title}: {text}");
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&ns(title));
    alert.setInformativeText(&ns(text));
    alert.setAlertStyle(if error { NSAlertStyle::Critical } else { NSAlertStyle::Informational });
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    alert.runModal();
}

pub fn main_thread() -> Option<MainThreadMarker> {
    MainThreadMarker::new()
}
