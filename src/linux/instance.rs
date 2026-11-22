//! Одна копия и `--trigger` через абстрактный unix-сокет `zyrik-<uid>`.
//!
//! Абстрактный сокет живёт, пока жив процесс; файла на диске нет. Занят — Зырик уже запущен.
//! Тот же сокет принимает `zyrik --trigger` (бинд в конфиге Sway/Hyprland): короткая команда
//! `capture` → `Command::Capture`. Прав доступа у абстрактного сокета нет, поэтому каждое
//! подключение проверяем: команды принимаются только от того же пользователя (SO_PEERCRED).

use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::sync::mpsc::Sender;
use std::time::Duration;

use zyrik::platform::{Command, Problem, Shot};

use super::Event;

const CAPTURE: &[u8] = b"capture\n";

fn addr() -> io::Result<SocketAddr> {
    let uid = unsafe { libc::getuid() };
    SocketAddr::from_abstract_name(format!("zyrik-{uid}").as_bytes())
}

/// uid процесса на том конце сокета.
fn peer_uid(s: &UnixStream) -> Option<u32> {
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let r = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (r == 0).then_some(cred.uid)
}

/// Занять место единственной копии и слушать `--trigger`.
pub fn claim(tx: Sender<Event>) -> Result<(), Problem> {
    let listener = match UnixListener::bind_addr(&addr().map_err(|e| Problem::Os(e.to_string()))?) {
        Ok(l) => l,
        Err(e) if e.kind() == io::ErrorKind::AddrInUse => return Err(Problem::AlreadyRunning),
        Err(e) => return Err(Problem::Os(e.to_string())),
    };
    let me = unsafe { libc::getuid() };
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut s) = conn else { continue };
            if peer_uid(&s) != Some(me) {
                continue;
            }
            let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
            let mut buf = [0u8; 16];
            let n = s.read(&mut buf).unwrap_or(0);
            if &buf[..n] == CAPTURE {
                let _ = tx.send(Event::Cmd(Command::Capture(Shot::Region)));
                let _ = s.write_all(b"ok\n");
            }
        }
    });
    Ok(())
}

/// `zyrik --trigger`: попросить запущенный Зырик зырнуть. Ошибка — Зырик не запущен.
pub fn trigger() -> io::Result<()> {
    let mut s = UnixStream::connect_addr(&addr()?)?;
    s.set_read_timeout(Some(Duration::from_secs(2)))?;
    s.write_all(CAPTURE)?;
    let mut buf = [0u8; 3];
    s.read_exact(&mut buf)?;
    if &buf == b"ok\n" {
        Ok(())
    } else {
        Err(io::Error::other("неожиданный ответ"))
    }
}
