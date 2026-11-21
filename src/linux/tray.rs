//! Значок в трее: StatusNotifierItem по D-Bus сессии (KDE, XFCE, Cinnamon, GNOME с расширением
//! AppIndicator). D-Bus — только локальный unix-сокет: адрес шины с `tcp:` не принимаем.
//! Значок — из того же `assets/zyrik.ico`, что и в Windows.

use std::sync::mpsc::Sender;

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{MenuItem as KMenuItem, StandardItem};
use ksni::{Icon, ToolTip, Tray};

use zyrik::platform::{Command, MenuItem};

use super::Event;

static ICO: &[u8] = include_bytes!("../../assets/zyrik.ico");

/// 32-битные BMP-картинки из .ico → ARGB по байтам (A, R, G, B), как просит SNI.
pub fn icons() -> Vec<Icon> {
    let rd16 = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as usize;
    let rd32 = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
    let mut out = Vec::new();
    if ICO.len() < 6 {
        return out;
    }
    for e in 0..rd16(ICO, 4) {
        let d = 6 + 16 * e;
        let (w, size, off) = (ICO[d] as usize, rd32(ICO, d + 8), rd32(ICO, d + 12));
        let Some(bmp) = ICO.get(off..off + size) else { continue };
        // PNG (256 px) пропускаем: для трея хватает 16–64
        if w == 0 || bmp.len() < 40 || rd16(bmp, 14) != 32 {
            continue;
        }
        let pixels = &bmp[rd32(bmp, 0)..];
        if pixels.len() < w * w * 4 {
            continue;
        }
        let mut data = Vec::with_capacity(w * w * 4);
        for y in (0..w).rev() {
            for px in pixels[y * w * 4..(y + 1) * w * 4].as_chunks::<4>().0 {
                data.extend_from_slice(&[px[3], px[2], px[1], px[0]]);
            }
        }
        out.push(Icon { width: w as i32, height: w as i32, data });
    }
    out
}

pub struct ZTray {
    tx: Sender<Event>,
    menu: Vec<MenuItem>,
    tip: String,
    icons: Vec<Icon>,
}

impl Tray for ZTray {
    fn id(&self) -> String {
        "zyrik".into()
    }

    fn title(&self) -> String {
        "Zyrik".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip { title: self.tip.clone(), ..ToolTip::default() }
    }

    /// Щелчок по значку — первый пункт меню («Зырнуть»), как в Windows.
    fn activate(&mut self, _x: i32, _y: i32) {
        if let Some(m) = self.menu.first() {
            let _ = self.tx.send(Event::Cmd(m.command.clone()));
        }
    }

    fn menu(&self) -> Vec<KMenuItem<Self>> {
        self.menu
            .iter()
            .map(|m| {
                let cmd: Command = m.command.clone();
                StandardItem {
                    label: m.text.replace('_', "__"),
                    activate: Box::new(move |t: &mut ZTray| {
                        let _ = t.tx.send(Event::Cmd(cmd.clone()));
                    }),
                    ..StandardItem::default()
                }
                .into()
            })
            .collect()
    }

    /// Панель с треем пропала (перезапуск оболочки) — не выходим, ждём, когда появится снова.
    fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
        true
    }
}

/// Адрес шины сессии — только `unix:` (принцип I: никаких сетевых соединений).
pub fn bus_is_local() -> bool {
    match std::env::var("DBUS_SESSION_BUS_ADDRESS") {
        Ok(a) => a.split(';').filter(|s| !s.is_empty()).all(|s| s.starts_with("unix:")),
        // без переменной zbus идёт в $XDG_RUNTIME_DIR/bus — это тоже unix-сокет
        Err(_) => true,
    }
}

pub fn spawn(tx: Sender<Event>, tip: &str, menu: &[MenuItem]) -> Result<Handle<ZTray>, String> {
    if !bus_is_local() {
        return Err("DBUS_SESSION_BUS_ADDRESS не unix: — значок в трее отключён".into());
    }
    let t = ZTray { tx, menu: menu.to_vec(), tip: tip.to_string(), icons: icons() };
    t.assume_sni_available(true).spawn().map_err(|e| e.to_string())
}

pub fn set_tip(h: &Handle<ZTray>, tip: &str) {
    let tip = tip.to_string();
    h.update(move |t| t.tip = tip);
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_sizes_from_ico() {
        let sizes: Vec<i32> = super::icons().iter().map(|i| i.width).collect();
        assert!(sizes.contains(&32) && sizes.contains(&64), "{sizes:?}");
        let i = super::icons().into_iter().find(|i| i.width == 32).unwrap();
        assert_eq!(i.data.len(), 32 * 32 * 4);
        // в центре значка — непрозрачная плашка
        let c = (16 * 32 + 16) * 4;
        assert_eq!(i.data[c], 0xFF);
    }
}
