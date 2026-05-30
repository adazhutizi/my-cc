use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tokio::sync::{mpsc, oneshot};

use crate::bus::EventBus;

pub struct PtyProcess {
    child: Mutex<Option<Box<dyn portable_pty::Child + Send>>>,
    killer: Mutex<Option<Box<dyn ChildKiller + Send + Sync>>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
}

impl PtyProcess {
    pub fn spawn(command: &str, args: &[String], rows: u16, cols: u16) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Windows 上通过 PowerShell -Command 包装命令，
        // 使 .ps1/.cmd/.bat 等非原生可执行文件都能按 PowerShell 的优先级正确解析
        #[cfg(windows)]
        let mut cmd = {
            let mut cmd = CommandBuilder::new("powershell");
            cmd.arg("-NoLogo");
            cmd.arg("-Command");
            cmd.arg(command);
            cmd.args(args);
            cmd
        };

        #[cfg(unix)]
        let mut cmd = {
            let mut cmd = CommandBuilder::new(command);
            cmd.args(args);
            cmd
        };

        cmd.cwd(std::env::current_dir()?);
        for (k, v) in std::env::vars() {
            cmd.env(&k, &v);
        }

        let child = pair.slave.spawn_command(cmd)?;
        let killer = child.clone_killer();

        Ok(PtyProcess {
            child: Mutex::new(Some(child)),
            killer: Mutex::new(Some(killer)),
            master: Mutex::new(pair.master),
        })
    }

    pub fn start_reader(&self, bus: &EventBus, shutdown: oneshot::Sender<()>) -> Result<()> {
        let mut reader = self.master.lock().unwrap().try_clone_reader()?;
        let sender = bus.output_sender_clone();
        let bus = bus.clone_inner();

        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let event = bus.record_output(&buf[..n]);
                        let _ = sender.send(event);
                    }
                    Err(_) => break,
                }
            }
            let _ = shutdown.send(());
        });

        Ok(())
    }

    pub fn start_exit_watcher(&self, shutdown: oneshot::Sender<()>) -> Result<()> {
        let mut child = self
            .child
            .lock()
            .unwrap()
            .take()
            .expect("child exit watcher already started");

        std::thread::spawn(move || {
            if let Err(e) = child.wait() {
                log::warn!("child wait failed: {e}");
            }
            let _ = shutdown.send(());
        });

        Ok(())
    }

    pub fn start_writer(&self, input_rx: mpsc::Receiver<Vec<u8>>) {
        let writer = self
            .master
            .lock()
            .unwrap()
            .take_writer()
            .expect("take PTY writer");
        let writer = Arc::new(Mutex::new(writer));

        std::thread::spawn(move || {
            let mut input_rx = input_rx;
            while let Some(data) = input_rx.blocking_recv() {
                let mut w = writer.lock().unwrap();
                if w.write_all(&data).is_err() {
                    break;
                }
            }
        });
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        let master = self.master.lock().unwrap();
        master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }
}

impl Drop for PtyProcess {
    fn drop(&mut self) {
        if let Some(mut killer) = self.killer.lock().unwrap().take() {
            let _ = killer.kill();
        }
    }
}
