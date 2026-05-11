// Binary message protocol

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Output = 0x01,
    Input = 0x02,
    Resize = 0x03,
    Mouse = 0x05,
    FeatureToggle = 0x06,
    Ping = 0x07,
    Pong = 0x08,
    ReplayEnd = 0x09,
    ReplayMode = 0x0A,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub msg_type: MessageType,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn output(seq: u64, data: Vec<u8>) -> Self {
        let mut payload = Vec::with_capacity(8 + data.len());
        payload.extend_from_slice(&seq.to_be_bytes());
        payload.extend_from_slice(&data);
        Message {
            msg_type: MessageType::Output,
            payload,
        }
    }

    #[allow(dead_code)]
    pub fn parse_output(&self) -> Option<(u64, &[u8])> {
        if self.payload.len() < 8 {
            return None;
        }
        let seq = u64::from_be_bytes([
            self.payload[0],
            self.payload[1],
            self.payload[2],
            self.payload[3],
            self.payload[4],
            self.payload[5],
            self.payload[6],
            self.payload[7],
        ]);
        Some((seq, &self.payload[8..]))
    }

    pub fn replay_mode(full: bool) -> Self {
        Message {
            msg_type: MessageType::ReplayMode,
            payload: vec![if full { 1 } else { 0 }],
        }
    }

    #[allow(dead_code)]
    pub fn input(data: Vec<u8>) -> Self {
        Message {
            msg_type: MessageType::Input,
            payload: data,
        }
    }

    pub fn resize(rows: u16, cols: u16) -> Self {
        let mut payload = Vec::with_capacity(4);
        payload.extend_from_slice(&rows.to_be_bytes());
        payload.extend_from_slice(&cols.to_be_bytes());
        Message {
            msg_type: MessageType::Resize,
            payload,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + self.payload.len());
        buf.push(self.msg_type as u8);
        buf.extend_from_slice(&self.payload);
        buf
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.is_empty() {
            return None;
        }
        let msg_type = match data[0] {
            0x01 => MessageType::Output,
            0x02 => MessageType::Input,
            0x03 => MessageType::Resize,
            0x05 => MessageType::Mouse,
            0x06 => MessageType::FeatureToggle,
            0x07 => MessageType::Ping,
            0x08 => MessageType::Pong,
            0x09 => MessageType::ReplayEnd,
            0x0A => MessageType::ReplayMode,
            _ => return None,
        };
        Some(Message {
            msg_type,
            payload: data[1..].to_vec(),
        })
    }

    pub fn parse_resize(&self) -> Option<(u16, u16)> {
        if self.payload.len() != 4 {
            return None;
        }
        let rows = u16::from_be_bytes([self.payload[0], self.payload[1]]);
        let cols = u16::from_be_bytes([self.payload[2], self.payload[3]]);
        Some((rows, cols))
    }
}
