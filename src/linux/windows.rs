//! Окна X11 в момент снимка (история 5): стопка окон EWMH, активное окно и курсор.
//! Только свойства окон и геометрия — содержимое окон отдельно не читается.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, MapState, Window};
use x11rb::rust_connection::RustConnection;

use zyrik::platform::Desktop;
use zyrik::session::{Pt, Rect};

use super::X;

fn atom(conn: &RustConnection, name: &str) -> u32 {
    conn.intern_atom(true, name.as_bytes()).ok().and_then(|c| c.reply().ok()).map_or(0, |r| r.atom)
}

/// Значения CARDINAL/WINDOW/ATOM-свойства (32 бита).
fn prop32(conn: &RustConnection, w: Window, name: u32, ty: impl Into<u32>, len: u32) -> Vec<u32> {
    if name == 0 {
        return Vec::new();
    }
    conn.get_property(false, w, name, ty, 0, len)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().map(Iterator::collect))
        .unwrap_or_default()
}

pub fn desktop(x: &X) -> Desktop {
    let conn = &x.conn;
    let stacking = atom(conn, "_NET_CLIENT_LIST_STACKING");
    let frame_ext = atom(conn, "_NET_FRAME_EXTENTS");
    let gtk_ext = atom(conn, "_GTK_FRAME_EXTENTS");
    let state = atom(conn, "_NET_WM_STATE");
    let hidden = atom(conn, "_NET_WM_STATE_HIDDEN");
    let wtype = atom(conn, "_NET_WM_WINDOW_TYPE");
    let wdesktop = atom(conn, "_NET_WM_WINDOW_TYPE_DESKTOP");

    // снизу вверх; без EWMH — дети корня (тоже снизу вверх)
    let mut clients = prop32(conn, x.root, stacking, AtomEnum::WINDOW, 4096);
    let ewmh = !clients.is_empty();
    if !ewmh {
        clients = conn.query_tree(x.root).ok().and_then(|c| c.reply().ok()).map(|r| r.children).unwrap_or_default();
    }

    let bounds = |w: Window| -> Option<Rect> {
        let attrs = conn.get_window_attributes(w).ok()?.reply().ok()?;
        if attrs.map_state != MapState::VIEWABLE || (!ewmh && attrs.override_redirect) || w == x.overlay {
            return None;
        }
        if prop32(conn, w, state, AtomEnum::ATOM, 64).contains(&hidden) && hidden != 0 {
            return None;
        }
        // рабочий стол — не окно: щелчок по нему начинает обычное выделение
        if prop32(conn, w, wtype, AtomEnum::ATOM, 16).contains(&wdesktop) && wdesktop != 0 {
            return None;
        }
        let g = conn.get_geometry(w).ok()?.reply().ok()?;
        let t = conn.translate_coordinates(w, x.root, 0, 0).ok()?.reply().ok()?;
        let (mut l, mut tp) = (i32::from(t.dst_x), i32::from(t.dst_y));
        let (mut r, mut b) = (l + i32::from(g.width), tp + i32::from(g.height));
        // рамка оконного менеджера — часть окна; тень клиентских рамок GTK — нет
        if let [el, er, et, eb] = prop32(conn, w, frame_ext, AtomEnum::CARDINAL, 4)[..] {
            l -= el as i32;
            r += er as i32;
            tp -= et as i32;
            b += eb as i32;
        }
        if let [gl, gr, gt, gb] = prop32(conn, w, gtk_ext, AtomEnum::CARDINAL, 4)[..] {
            l += gl as i32;
            r -= gr as i32;
            tp += gt as i32;
            b -= gb as i32;
        }
        (r > l && b > tp).then_some(Rect { x: l, y: tp, w: r - l, h: b - tp })
    };

    let windows: Vec<Rect> = clients.iter().rev().filter_map(|&w| bounds(w)).collect();
    let active = prop32(conn, x.root, x.atoms.net_active_window, AtomEnum::WINDOW, 1)
        .first()
        .copied()
        .filter(|&w| w != 0)
        .and_then(bounds);
    let cursor = conn
        .query_pointer(x.root)
        .ok()
        .and_then(|c| c.reply().ok())
        .filter(|p| p.same_screen)
        .map(|p| Pt { x: i32::from(p.root_x), y: i32::from(p.root_y) });
    let _ = conn.flush();
    Desktop { windows, active, cursor }
}
