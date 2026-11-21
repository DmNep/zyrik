//! FR-015: одна копия на сеанс пользователя. Именованный мьютекс живёт только в памяти ядра ОС —
//! ни файла, ни записи в реестре.

use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Threading::CreateMutexW;

use zyrik::platform::Problem;

pub struct Guard(HANDLE);

impl Guard {
    pub fn claim() -> Result<Guard, Problem> {
        unsafe {
            let h = CreateMutexW(None, true, w!("Local\\Zyrik.SingleInstance"))
                .map_err(|e| Problem::Os(e.message()))?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(h);
                return Err(Problem::AlreadyRunning);
            }
            Ok(Guard(h))
        }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
