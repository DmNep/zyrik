//! Подключение к X-серверу — только через локальный unix-сокет (принцип I: ни одного сетевого
//! соединения). `DISPLAY` с именем машины (`host:0`, X по сети/SSH-туннель) не поддерживаем.

use std::io;
use std::os::unix::net::UnixStream;

use x11rb::reexports::x11rb_protocol::xauth;
use x11rb::rust_connection::{DefaultStream, RustConnection};

/// `:0`, `:1.0`, `unix:0` → (номер дисплея, экран). Всё с именем машины — `None`.
pub fn parse_display(s: &str) -> Option<(u16, usize)> {
    let rest = s.strip_prefix("unix").unwrap_or(s);
    let rest = rest.strip_prefix(':')?;
    let (num, screen) = match rest.split_once('.') {
        Some((n, sc)) => (n, sc.parse().ok()?),
        None => (rest, 0),
    };
    Some((num.parse().ok()?, screen))
}

fn unix_stream(display: u16) -> io::Result<UnixStream> {
    let path = format!("/tmp/.X11-unix/X{display}");
    #[cfg(target_os = "linux")]
    {
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::SocketAddr;
        // сначала абстрактный сокет (так подключается libxcb), потом файл сокета
        if let Ok(addr) = SocketAddr::from_abstract_name(path.as_bytes()) {
            if let Ok(s) = UnixStream::connect_addr(&addr) {
                return Ok(s);
            }
        }
    }
    UnixStream::connect(path)
}

/// Ошибка подключения — понятным текстом для сообщения.
pub fn connect() -> Result<(RustConnection, usize), String> {
    let name = std::env::var("DISPLAY").map_err(|_| "DISPLAY не задан: нужен запущенный X-сервер (X11)".to_string())?;
    let (display, screen) =
        parse_display(&name).ok_or_else(|| format!("DISPLAY={name}: Зырик работает только с локальным дисплеем"))?;
    let stream = unix_stream(display).map_err(|e| format!("DISPLAY={name}: {e}"))?;
    let (stream, (family, address)) = DefaultStream::from_unix_stream(stream).map_err(|e| e.to_string())?;
    // ~/.Xauthority или $XAUTHORITY — только чтение
    let (auth_name, auth_data) = xauth::get_auth(family, &address, display).ok().flatten().unwrap_or_default();
    let conn = RustConnection::connect_to_stream_with_auth_info(stream, screen, auth_name, auth_data)
        .map_err(|e| format!("DISPLAY={name}: {e}"))?;
    Ok((conn, screen))
}

#[cfg(test)]
mod tests {
    use super::parse_display;

    #[test]
    fn only_local_displays() {
        assert_eq!(parse_display(":0"), Some((0, 0)));
        assert_eq!(parse_display(":1.2"), Some((1, 2)));
        assert_eq!(parse_display("unix:3"), Some((3, 0)));
        assert_eq!(parse_display("localhost:10.0"), None, "через ssh -X — сеть");
        assert_eq!(parse_display("10.0.0.5:0"), None);
        assert_eq!(parse_display(""), None);
    }
}
