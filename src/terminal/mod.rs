#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use anyhow::Result;
use crossterm::terminal;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::bus::EventBus;

#[cfg(unix)]
use unix::{spawn_input_reader, spawn_resize_watcher};
#[cfg(windows)]
use windows::{spawn_input_reader, spawn_resize_watcher};

pub struct TerminalGuard {
    #[cfg(windows)]
    input_mode: Option<windows::InputModeGuard>,
}

impl TerminalGuard {
    pub fn enter_raw_mode() -> Result<Self> {
        terminal::enable_raw_mode()?;
        #[cfg(windows)]
        let input_mode = windows::enable_virtual_terminal_input()
            .map(Some)
            .map_err(anyhow::Error::from)?;

        Ok(TerminalGuard {
            #[cfg(windows)]
            input_mode,
        })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(input_mode) = self.input_mode.take() {
            input_mode.restore();
        }

        let _ = terminal::disable_raw_mode();
    }
}

pub fn get_terminal_size() -> (u16, u16) {
    terminal::size()
        .map(|(cols, rows)| (rows, cols))
        .unwrap_or((24, 80))
}

pub async fn run_local_terminal(bus: EventBus) -> Result<()> {
    let mut output_rx = bus.subscribe_output();
    let mut stdout = tokio::io::stdout();

    let output_handle = tokio::spawn(async move {
        while let Ok(event) = output_rx.recv().await {
            if stdout.write_all(&event.data).await.is_err() {
                break;
            }
            let _ = stdout.flush().await;
        }
    });

    let input_handle = spawn_input_reader(bus.input_sender());
    let resize_handle = spawn_resize_watcher(bus.clone_inner());

    tokio::select! {
        _ = output_handle => {}
        _ = input_handle => {}
    }
    resize_handle.abort();
    Ok(())
}

fn spawn_stdin_byte_reader(input_sender: mpsc::Sender<Vec<u8>>) -> JoinHandle<()> {
    use tokio::io::AsyncReadExt;

    tokio::spawn(async move {
        let mut stdin = tokio::io::stdin();
        let mut buf = [0u8; 256];
        loop {
            match stdin.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    if input_sender.send(buf[..n].to_vec()).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    })
}
