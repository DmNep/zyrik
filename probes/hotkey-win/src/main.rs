//! R1: ловится ли Ctrl+PrtScr через RegisterHotKey на Windows 11.
//!
//! Запуск:
//!   hotkey-win            — самопроверка: регистрирует клавиши, шлёт Ctrl+PrtScr через SendInput, ждёт 2 с
//!   hotkey-win --listen N — регистрирует и N секунд ждёт реальных нажатий (проверка руками)
//!
//! Ничего не пишет ни на диск, ни в реестр, ни в буфер обмена.

use std::time::Instant;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const KEYS: [(i32, HOT_KEY_MODIFIERS, &str); 3] = [
    (1, MOD_CONTROL, "Ctrl+PrtScr"),
    (2, MOD_SHIFT, "Shift+PrtScr"),
    (3, HOT_KEY_MODIFIERS(0), "PrtScr"),
];

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let listen = args.iter().position(|a| a == "--listen")
        .map(|i| args.get(i + 1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(30));

    let snip = std::process::Command::new("reg")
        .args(["query", r"HKCU\Control Panel\Keyboard", "/v", "PrintScreenKeyForSnippingEnabled"])
        .output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("0x1"));
    println!("«PrtScr открывает Ножницы»: {}", match snip { Some(true) => "ВКЛ", Some(false) => "выкл", None => "?" });

    let mut ctrl_ok = false;
    for (id, m, name) in KEYS {
        let r = unsafe { RegisterHotKey(None, id, m | MOD_NOREPEAT, VK_SNAPSHOT.0 as u32) };
        match &r {
            Ok(()) => println!("RegisterHotKey {name:<13} → OK"),
            Err(e) => println!("RegisterHotKey {name:<13} → ОШИБКА {:#x} {}", e.code().0, e.message()),
        }
        if id == 1 { ctrl_ok = r.is_ok(); }
    }

    let wait_ms = match listen {
        Some(s) => { println!("Жду реальных нажатий {s} с…"); s * 1000 }
        None => {
            if !ctrl_ok { println!("Ctrl+PrtScr не зарегистрирован — самопроверку не шлю"); return; }
            let seq = [key(VK_CONTROL, false), key(VK_SNAPSHOT, false), key(VK_SNAPSHOT, true), key(VK_CONTROL, true)];
            let sent = unsafe { SendInput(&seq, std::mem::size_of::<INPUT>() as i32) };
            println!("SendInput Ctrl+PrtScr: отправлено {sent}/4 событий");
            2000
        }
    };

    let t0 = Instant::now();
    let timer = unsafe { SetTimer(None, 0, wait_ms, None) };
    let mut hits = [0u32; 4];
    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        match msg.message {
            WM_HOTKEY => {
                let id = msg.wParam.0;
                if id < hits.len() { hits[id] += 1; }
                let name = KEYS.iter().find(|k| k.0 as usize == id).map_or("?", |k| k.2);
                println!("  WM_HOTKEY {name} через {} мс", t0.elapsed().as_millis());
            }
            WM_TIMER if msg.wParam.0 == timer => break,
            _ => {}
        }
    }
    unsafe {
        let _ = KillTimer(None, timer);
        for (id, _, _) in KEYS { let _ = UnregisterHotKey(None, id); }
    }

    if listen.is_none() {
        println!("ИТОГ: Ctrl+PrtScr {}", if hits[1] > 0 { "ловится" } else { "НЕ ловится" });
        std::process::exit(if hits[1] > 0 { 0 } else { 1 });
    }
    println!("ИТОГ: Ctrl+PrtScr {}, Shift+PrtScr {}, PrtScr {}", hits[1], hits[2], hits[3]);
}
