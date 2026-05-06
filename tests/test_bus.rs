use my_cc::bus::EventBus;

#[tokio::test]
async fn broadcast_output_to_multiple_subscribers() {
    let bus = EventBus::new(64);
    let mut sub1 = bus.subscribe_output();
    let mut sub2 = bus.subscribe_output();

    bus.send_output(vec![1, 2, 3]);

    let data1 = sub1.try_recv().unwrap();
    let data2 = sub2.try_recv().unwrap();
    assert_eq!(data1.seq, 1);
    assert_eq!(data1.data, vec![1, 2, 3]);
    assert_eq!(data2.seq, 1);
    assert_eq!(data2.data, vec![1, 2, 3]);
}

#[tokio::test]
async fn output_replay_returns_current_mark_and_log() {
    let bus = EventBus::new(64);

    bus.send_output(vec![1]);
    bus.send_output(vec![2]);

    let (mark, data) = bus.output_replay().await;
    assert_eq!(mark, 2);
    assert_eq!(data, vec![vec![1], vec![2]]);
}

#[tokio::test]
async fn input_channel_receives_from_multiple_senders() {
    let bus = EventBus::new(64);
    let sender1 = bus.input_sender();
    let sender2 = bus.input_sender();

    sender1.send(vec![1]).await.unwrap();
    sender2.send(vec![2]).await.unwrap();

    let mut rx = bus.take_input_receiver().await.unwrap();
    assert_eq!(rx.recv().await.unwrap(), vec![1]);
    assert_eq!(rx.recv().await.unwrap(), vec![2]);
}

#[tokio::test]
async fn take_input_receiver_returns_none_on_second_call() {
    let bus = EventBus::new(64);
    let _rx = bus.take_input_receiver().await;
    let result = bus.take_input_receiver().await;
    assert!(result.is_none());
}

#[tokio::test]
async fn resize_broadcast() {
    let bus = EventBus::new(64);
    let mut sub = bus.subscribe_resize();

    bus.send_resize(50, 160);

    let (rows, cols) = sub.try_recv().unwrap();
    assert_eq!(rows, 50);
    assert_eq!(cols, 160);
    assert_eq!(bus.current_size(), Some((50, 160)));
}
