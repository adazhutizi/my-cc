use tokio::task::JoinHandle;
use tokio::time::{Duration, MissedTickBehavior};
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_INPUT, STD_INPUT_HANDLE,
};

use crate::bus::EventBus;

const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub fn spawn_resize_watcher(bus: EventBus) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(POLL_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        let mut last = super::get_terminal_size();
        loop {
            interval.tick().await;
            let cur = super::get_terminal_size();
            if cur != last {
                last = cur;
                let (rows, cols) = cur;
                bus.send_resize(rows, cols);
            }
        }
    })
}

pub fn spawn_input_reader(input_sender: tokio::sync::mpsc::Sender<Vec<u8>>) -> JoinHandle<()> {
    super::spawn_stdin_byte_reader(input_sender)
}

pub struct InputModeGuard {
    handle: windows_sys::Win32::Foundation::HANDLE,
    original_mode: u32,
}

impl InputModeGuard {
    pub fn restore(self) {
        unsafe {
            let _ = SetConsoleMode(self.handle, self.original_mode);
        }
    }
}

pub fn enable_virtual_terminal_input() -> std::io::Result<InputModeGuard> {
    unsafe {
        let handle = GetStdHandle(STD_INPUT_HANDLE);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }

        let mut original_mode = 0;
        if GetConsoleMode(handle, &mut original_mode) == 0 {
            return Err(std::io::Error::last_os_error());
        }

        let new_mode = original_mode | ENABLE_VIRTUAL_TERMINAL_INPUT;
        if SetConsoleMode(handle, new_mode) == 0 {
            return Err(std::io::Error::last_os_error());
        }

        Ok(InputModeGuard {
            handle,
            original_mode,
        })
    }
}
