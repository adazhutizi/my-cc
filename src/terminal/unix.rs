use tokio::signal::unix::{signal, SignalKind};
use tokio::task::JoinHandle;

use crate::bus::EventBus;

pub fn spawn_resize_watcher(bus: EventBus) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut sig = match signal(SignalKind::window_change()) {
            Ok(s) => s,
            Err(e) => {
                log::error!("failed to install SIGWINCH handler: {e}");
                return;
            }
        };
        loop {
            sig.recv().await;
            let (rows, cols) = super::get_terminal_size();
            bus.send_resize(rows, cols);
        }
    })
}

pub fn spawn_input_reader(input_sender: tokio::sync::mpsc::Sender<Vec<u8>>) -> JoinHandle<()> {
    super::spawn_stdin_byte_reader(input_sender)
}
