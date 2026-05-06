use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{broadcast, mpsc, Mutex};

#[derive(Debug, Clone)]
pub struct OutputEvent {
    pub seq: u64,
    pub data: Vec<u8>,
}

pub struct EventBus {
    output_tx: broadcast::Sender<OutputEvent>,
    output_log: Arc<StdMutex<Vec<OutputEvent>>>,
    output_seq: Arc<std::sync::atomic::AtomicU64>,
    input_tx: mpsc::Sender<Vec<u8>>,
    input_rx: Arc<Mutex<Option<mpsc::Receiver<Vec<u8>>>>>,
    resize_tx: broadcast::Sender<(u16, u16)>,
    current_size: Arc<StdMutex<Option<(u16, u16)>>>,
}

impl EventBus {
    pub fn new(buffer: usize) -> Self {
        let (output_tx, _) = broadcast::channel(buffer);
        let (input_tx, input_rx) = mpsc::channel(buffer);
        let (resize_tx, _) = broadcast::channel(16);
        EventBus {
            output_tx,
            output_log: Arc::new(StdMutex::new(Vec::with_capacity(buffer))),
            output_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            input_tx,
            input_rx: Arc::new(Mutex::new(Some(input_rx))),
            resize_tx,
            current_size: Arc::new(StdMutex::new(None)),
        }
    }

    #[allow(dead_code)]
    pub fn send_output(&self, data: Vec<u8>) {
        let event = self.record_output(&data);
        let _ = self.output_tx.send(event);
    }

    pub async fn output_replay(&self) -> (u64, Vec<Vec<u8>>) {
        let log = self.output_log.lock().unwrap();
        let mark = log.last().map(|event| event.seq).unwrap_or(0);
        let data = log.iter().map(|event| event.data.clone()).collect();
        (mark, data)
    }

    pub fn record_output(&self, data: &[u8]) -> OutputEvent {
        let seq = self
            .output_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let event = OutputEvent {
            seq,
            data: data.to_vec(),
        };
        let mut log = self.output_log.lock().unwrap();
        log.push(event.clone());
        if log.len() > 16384 {
            let excess = log.len() - 16384;
            log.drain(0..excess);
        }
        event
    }

    pub fn subscribe_output(&self) -> broadcast::Receiver<OutputEvent> {
        self.output_tx.subscribe()
    }

    pub fn input_sender(&self) -> mpsc::Sender<Vec<u8>> {
        self.input_tx.clone()
    }

    pub async fn take_input_receiver(&self) -> Option<mpsc::Receiver<Vec<u8>>> {
        self.input_rx.lock().await.take()
    }

    pub fn send_resize(&self, rows: u16, cols: u16) {
        *self.current_size.lock().unwrap() = Some((rows, cols));
        let _ = self.resize_tx.send((rows, cols));
    }

    pub fn current_size(&self) -> Option<(u16, u16)> {
        *self.current_size.lock().unwrap()
    }

    pub fn subscribe_resize(&self) -> broadcast::Receiver<(u16, u16)> {
        self.resize_tx.subscribe()
    }

    pub fn output_sender_clone(&self) -> broadcast::Sender<OutputEvent> {
        self.output_tx.clone()
    }

    pub fn clone_inner(&self) -> Self {
        EventBus {
            output_tx: self.output_tx.clone(),
            output_log: self.output_log.clone(),
            output_seq: self.output_seq.clone(),
            input_tx: self.input_tx.clone(),
            input_rx: self.input_rx.clone(),
            resize_tx: self.resize_tx.clone(),
            current_size: self.current_size.clone(),
        }
    }
}
