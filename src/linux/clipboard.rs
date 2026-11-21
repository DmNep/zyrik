//! Буфер обмена X11 (CLIPBOARD): владеем выделением и отдаём PNG по запросу — копия снимка
//! живёт только у нас в памяти, никакой менеджер буфера её не хранит, пока сам не попросит.
//!
//! - Цели: `TARGETS`, `image/png`, `x-kde-passwordManagerHint` = `secret` (Klipper и другие
//!   менеджеры буфера не кладут такое в историю).
//! - Одноразовость (FR-004a, `zyrik::oneshot`): «на переднем плане» — запрос от того же клиента X,
//!   что и активное окно (`_NET_ACTIVE_WINDOW`, сравнение по базе идентификаторов клиента);
//!   модификатор — Ctrl или Super по `QueryKeymap`. После вставки с Ctrl — ждём, пока отпустят,
//!   и отказываемся от выделения, снимок затираем.
//! - Большие картинки — по протоколу INCR кусками.

use std::sync::Arc;
use std::time::Duration;

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, PropMode, PropertyNotifyEvent, Property,
    SelectionNotifyEvent, SelectionRequestEvent, SELECTION_NOTIFY_EVENT,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{CURRENT_TIME, NONE};
use zeroize::Zeroizing;

use zyrik::export::encode_png;
use zyrik::image::Image;
use zyrik::oneshot::{Answer, OneShot};
use zyrik::platform::Problem;

use super::X;

/// Кусок INCR
const CHUNK: usize = 256 * 1024;
const WATCH: Duration = Duration::from_millis(30);

#[derive(Default)]
pub struct Clip {
    image: Option<Image>,
    png: Option<Zeroizing<Vec<u8>>>,
    oneshot: OneShot,
    watching: bool,
    /// Меняется при каждой новой картинке — старые наблюдатели узнают, что они не нужны
    generation: u64,
    transfers: Vec<Incr>,
}

struct Incr {
    requestor: u32,
    property: u32,
    sent: usize,
    generation: u64,
}

impl Clip {
    fn wipe(&mut self) {
        if let Some(mut img) = self.image.take() {
            img.wipe();
        }
        self.png = None; // Zeroizing затирает байты
        self.transfers.clear();
        self.watching = false;
    }

    fn png(&mut self) -> Option<&[u8]> {
        if self.png.is_none() {
            self.png = self.image.as_ref().map(encode_png);
        }
        self.png.as_deref().map(Vec::as_slice)
    }
}

fn os<E: std::fmt::Display>(e: E) -> Problem {
    Problem::Os(e.to_string())
}

pub fn set_image(x: &X, image: Image) -> Result<(), Problem> {
    {
        let mut c = x.clip.lock().unwrap();
        c.wipe();
        c.image = Some(image);
        c.oneshot = OneShot::new();
        c.generation += 1;
    }
    let a = &x.atoms;
    x.conn.set_selection_owner(x.owner, a.clipboard, CURRENT_TIME).map_err(os)?;
    let owner = x.conn.get_selection_owner(a.clipboard).map_err(os)?.reply().map_err(os)?.owner;
    if owner != x.owner {
        x.clip.lock().unwrap().wipe();
        return Err(Problem::Os("CLIPBOARD: не удалось стать владельцем".into()));
    }
    Ok(())
}

/// Кто-то другой положил в буфер своё — наш снимок больше не нужен.
pub fn lost(x: &X) {
    x.clip.lock().unwrap().wipe();
}

fn in_front(x: &X, requestor: u32) -> bool {
    let reply = x
        .conn
        .get_property(false, x.root, x.atoms.net_active_window, AtomEnum::WINDOW, 0, 1)
        .ok()
        .and_then(|c| c.reply().ok());
    let Some(active) = reply.and_then(|r| r.value32().and_then(|mut v| v.next())) else {
        return false; // без оконного менеджера «переднего плана» нет — отдаём только с Ctrl
    };
    let mask = x.conn.setup().resource_id_mask;
    active != 0 && (active & !mask) == (requestor & !mask)
}

fn modifier_held(x: &X) -> bool {
    let Some(keys) = x.conn.query_keymap().ok().and_then(|c| c.reply().ok()) else { return false };
    x.keymap.lock().unwrap().ctrl_or_super_down(&keys.keys)
}

fn notify(x: &X, ev: &SelectionRequestEvent, property: u32) {
    let n = SelectionNotifyEvent {
        response_type: SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: ev.time,
        requestor: ev.requestor,
        selection: ev.selection,
        target: ev.target,
        property,
    };
    let _ = x.conn.send_event(false, ev.requestor, EventMask::NO_EVENT, n);
    let _ = x.conn.flush();
}

pub fn request(x: &Arc<X>, ev: SelectionRequestEvent) {
    let a = &x.atoms;
    let property = if ev.property == NONE { ev.target } else { ev.property };
    let mut c = x.clip.lock().unwrap();
    if ev.selection != a.clipboard || ev.owner != x.owner || c.image.is_none() {
        drop(c);
        return notify(x, &ev, NONE);
    }
    let ok = if ev.target == a.targets {
        let list = [a.targets, a.image_png, a.kde_hint];
        x.conn
            .change_property32(PropMode::REPLACE, ev.requestor, property, AtomEnum::ATOM, &list)
            .is_ok()
    } else if ev.target == a.kde_hint {
        x.conn.change_property8(PropMode::REPLACE, ev.requestor, property, a.kde_hint, b"secret").is_ok()
    } else if ev.target == a.image_png {
        let answer = c.oneshot.requested(in_front(x, ev.requestor), modifier_held(x));
        if answer == Answer::Refuse {
            false
        } else {
            let sent = send_png(x, &mut c, ev.requestor, property);
            if sent && answer == Answer::GiveAndWatch && !c.watching {
                c.watching = true;
                watch(Arc::clone(x), c.generation);
            }
            sent
        }
    } else {
        false
    };
    drop(c);
    notify(x, &ev, if ok { property } else { NONE });
}

fn send_png(x: &X, c: &mut Clip, requestor: u32, property: u32) -> bool {
    let limit = x.conn.maximum_request_bytes().saturating_sub(1024).min(4 << 20);
    let a_png = x.atoms.image_png;
    let incr = x.atoms.incr;
    let generation = c.generation;
    let Some(png) = c.png() else { return false };
    if png.len() <= limit {
        return x.conn.change_property8(PropMode::REPLACE, requestor, property, a_png, png).is_ok();
    }
    // большой снимок: INCR — объявляем размер, дальше куски по удалению свойства
    let len = png.len() as u32;
    let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::PROPERTY_CHANGE);
    if x.conn.change_window_attributes(requestor, &aux).is_err() {
        return false;
    }
    if x.conn.change_property32(PropMode::REPLACE, requestor, property, incr, &[len]).is_err() {
        return false;
    }
    c.transfers.retain(|t| !(t.requestor == requestor && t.property == property));
    c.transfers.push(Incr { requestor, property, sent: 0, generation });
    true
}

/// Получатель забрал кусок (удалил свойство) — шлём следующий; пустой кусок — конец.
pub fn property(x: &X, ev: &PropertyNotifyEvent) {
    if ev.state != Property::DELETE {
        return;
    }
    let mut c = x.clip.lock().unwrap();
    let Some(i) = c.transfers.iter().position(|t| t.window_matches(ev)) else { return };
    let t = &c.transfers[i];
    let (requestor, property, sent, generation) = (t.requestor, t.property, t.sent, t.generation);
    let (end, done) = {
        // PNG уже закодирован в начале INCR; снимок заменили — дошлём пустой кусок
        let data: &[u8] = if generation == c.generation { c.png.as_deref().map_or(&[], Vec::as_slice) } else { &[] };
        let end = (sent + CHUNK).min(data.len());
        let chunk = &data[sent.min(end)..end];
        let _ = x.conn.change_property8(PropMode::REPLACE, requestor, property, x.atoms.image_png, chunk);
        (end, chunk.is_empty())
    };
    if done {
        c.transfers.remove(i);
        let aux = ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT);
        let _ = x.conn.change_window_attributes(requestor, &aux);
    } else {
        c.transfers[i].sent = end;
    }
    let _ = x.conn.flush();
}

impl Incr {
    fn window_matches(&self, ev: &PropertyNotifyEvent) -> bool {
        self.requestor == ev.window && self.property == ev.atom
    }
}

/// Вставили с зажатым Ctrl/Super: как отпустят — отказываемся от буфера и затираем снимок.
fn watch(x: Arc<X>, generation: u64) {
    std::thread::spawn(move || loop {
        std::thread::sleep(WATCH);
        let held = modifier_held(&x);
        let mut c = x.clip.lock().unwrap();
        if c.generation != generation || c.image.is_none() {
            return;
        }
        if c.oneshot.should_clear(held) {
            c.wipe();
            drop(c);
            let a = x.atoms.clipboard;
            let still_ours = x.conn.get_selection_owner(a).ok().and_then(|r| r.reply().ok()).map(|r| r.owner) == Some(x.owner);
            if still_ours {
                let _ = x.conn.set_selection_owner(NONE, a, CURRENT_TIME);
                let _ = x.conn.flush();
            }
            return;
        }
    });
}
