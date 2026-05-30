mod bus;
mod protocol;
mod pty;
mod server;
mod terminal;

use std::sync::Arc;

use anyhow::Result;
use bus::EventBus;
use clap::Parser;
use pty::PtyProcess;
use server::run_server;
use terminal::{get_terminal_size, run_local_terminal, TerminalGuard};

#[cfg(unix)]
const DEFAULT_SHELL: &str = "/bin/sh";
#[cfg(windows)]
const DEFAULT_SHELL: &str = "powershell";

#[cfg(unix)]
fn default_command() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| DEFAULT_SHELL.to_string())
}

#[cfg(windows)]
fn default_command() -> String {
    DEFAULT_SHELL.to_string()
}

#[derive(Parser)]
#[command(name = "my-cc", about = "Terminal proxy — local + web", version)]
struct Cli {
    /// Command to run (default: $SHELL)
    command: Option<String>,

    /// Arguments for the command
    #[arg(last = true)]
    args: Vec<String>,

    /// Web server port
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Authentication token for web connections
    #[arg(long)]
    token: Option<String>,
}

fn main() -> Result<()> {
    env_logger::init();

    let cli = Cli::parse();
    let command = cli.command.unwrap_or_else(default_command);
    let (rows, cols) = get_terminal_size();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async move {
        let bus = EventBus::new(16384);
        bus.send_resize(rows, cols);

        // Spawn PTY
        let pty = PtyProcess::spawn(&command, &cli.args, rows, cols)?;
        let pty = Arc::new(pty);
        let (reader_shutdown_tx, reader_shutdown_rx) = tokio::sync::oneshot::channel();
        let (exit_shutdown_tx, exit_shutdown_rx) = tokio::sync::oneshot::channel();
        pty.start_reader(&bus, reader_shutdown_tx)?;
        pty.start_exit_watcher(exit_shutdown_tx)?;
        let input_rx = bus
            .take_input_receiver()
            .await
            .expect("input receiver already taken");
        pty.start_writer(input_rx);

        // Resize listener: forward resize events to PTY
        // Uses std::thread because PtyProcess is not Send (portable_pty types lack Sync)
        let mut resize_rx = bus.subscribe_resize();
        let pty_resize = Arc::clone(&pty);
        std::thread::spawn(move || {
            while let Ok((rows, cols)) = resize_rx.blocking_recv() {
                if let Err(e) = pty_resize.resize(rows, cols) {
                    log::warn!("resize failed: {e}");
                }
            }
        });

        // Web server
        let server_bus = bus.clone_inner();
        let port = cli.port;
        let token = cli.token;
        tokio::spawn(async move {
            if let Err(e) = run_server(port, server_bus, token).await {
                log::error!("server error: {e}");
            }
        });

        // Local terminal — exit when child process exits
        let _guard = TerminalGuard::enter_raw_mode()?;
        tokio::select! {
            result = run_local_terminal(bus) => { let _ = result; }
            _ = reader_shutdown_rx => {}
            _ = exit_shutdown_rx => {}
        }

        Ok(())
    })
}
