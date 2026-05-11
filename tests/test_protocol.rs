use my_cc::protocol::{Message, MessageType};

#[test]
fn encode_decode_output() {
    let msg = Message::output(42, vec![0x41, 0x42, 0x43]);
    let encoded = msg.encode();
    assert_eq!(encoded[0], 0x01);
    assert_eq!(encoded.len(), 12); // 1 type + 8 seq + 3 data

    let decoded = Message::decode(&encoded).unwrap();
    assert!(matches!(decoded.msg_type, MessageType::Output));
    let (seq, data) = decoded.parse_output().unwrap();
    assert_eq!(seq, 42);
    assert_eq!(data, &[0x41, 0x42, 0x43]);
}

#[test]
fn encode_decode_input() {
    let msg = Message::input(vec![0x0D]);
    let encoded = msg.encode();
    assert_eq!(encoded[0], 0x02);

    let decoded = Message::decode(&encoded).unwrap();
    assert!(matches!(decoded.msg_type, MessageType::Input));
    assert_eq!(decoded.payload, vec![0x0D]);
}

#[test]
fn encode_decode_resize() {
    let msg = Message::resize(50, 160);
    let encoded = msg.encode();
    assert_eq!(encoded[0], 0x03);
    assert_eq!(encoded.len(), 5); // 1 type + 4 bytes (2x u16 BE)

    let decoded = Message::decode(&encoded).unwrap();
    assert!(matches!(decoded.msg_type, MessageType::Resize));
    let (rows, cols) = decoded.parse_resize().unwrap();
    assert_eq!(rows, 50);
    assert_eq!(cols, 160);
}

#[test]
fn decode_empty_returns_none() {
    assert!(Message::decode(&[]).is_none());
}

#[test]
fn decode_unknown_type_returns_none() {
    assert!(Message::decode(&[0xFF, 0x00]).is_none());
}

#[test]
fn encode_decode_ping_pong() {
    let ping = Message {
        msg_type: MessageType::Ping,
        payload: vec![],
    };
    let encoded = ping.encode();
    assert_eq!(encoded[0], 0x07);
    let decoded = Message::decode(&encoded).unwrap();
    assert!(matches!(decoded.msg_type, MessageType::Ping));

    let pong = Message {
        msg_type: MessageType::Pong,
        payload: vec![],
    };
    let encoded = pong.encode();
    assert_eq!(encoded[0], 0x08);
    let decoded = Message::decode(&encoded).unwrap();
    assert!(matches!(decoded.msg_type, MessageType::Pong));
}
