use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_tungstenite::tungstenite::Message;
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use tokio::net::TcpListener;

use crate::bus::EventBus;
use crate::protocol;

static INDEX_HTML: &str = include_str!("../static/index.html");

type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub async fn run_server(mut port: u16, bus: EventBus, token: Option<String>) -> Result<()> {
    let listener = loop {
        let addr = format!("0.0.0.0:{port}");
        match TcpListener::bind(&addr).await {
            Ok(l) => {
                log::info!("HTTP/WS server listening on {addr}");
                break l;
            }
            Err(_) if port < u16::MAX => {
                log::info!("端口 {port} 已被占用，尝试 {port} + 1");
                port += 1;
            }
            Err(e) => return Err(e.into()),
        }
    };

    let mut http = hyper::server::conn::http1::Builder::new();
    http.keep_alive(true);

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let bus = bus.clone_inner();
        let token = token.clone();
        let http = http.clone();

        tokio::spawn(async move {
            let service = service_fn(move |req| {
                let bus = bus.clone_inner();
                let token = token.clone();
                async move {
                    match handle_request(req, bus, token).await {
                        Ok(resp) => Ok::<_, Infallible>(resp),
                        Err(e) => {
                            log::debug!("request error: {e}");
                            Ok::<_, Infallible>(
                                Response::builder()
                                    .status(500)
                                    .body(Full::new(Bytes::from("internal error")))
                                    .unwrap(),
                            )
                        }
                    }
                }
            });

            let conn = http.serve_connection(io, service).with_upgrades();
            if let Err(e) = conn.await {
                log::debug!("connection error: {e}");
            }
        });
    }
}

async fn handle_request(
    mut req: Request<hyper::body::Incoming>,
    bus: EventBus,
    token: Option<String>,
) -> Result<Response<Full<Bytes>>, BoxError> {
    if hyper_tungstenite::is_upgrade_request(&req) {
        if let Some(ref expected) = token {
            let provided = req.uri().query().and_then(|q| {
                q.split('&')
                    .find(|p| p.starts_with("token="))
                    .map(|p| &p[6..])
            });
            match provided {
                Some(t) if t == expected => {}
                _ => {
                    return Ok(Response::builder()
                        .status(403)
                        .body(Full::new(Bytes::from("forbidden")))
                        .unwrap());
                }
            }
        }

        let last_seq = req.uri().query().and_then(|q| {
            q.split('&')
                .find(|p| p.starts_with("lastSeq="))
                .and_then(|p| p[8..].parse::<u64>().ok())
        });

        let (response, websocket) = hyper_tungstenite::upgrade(&mut req, None)?;

        tokio::spawn(async move {
            if let Err(e) = handle_websocket(websocket, bus, last_seq).await {
                log::debug!("websocket error: {e}");
            }
        });

        Ok(response)
    } else {
        Ok(Response::new(Full::new(Bytes::from(INDEX_HTML))))
    }
}

async fn handle_websocket(
    websocket: hyper_tungstenite::HyperWebsocket,
    bus: EventBus,
    last_seq: Option<u64>,
) -> Result<()> {
    let mut ws = websocket.await?;

    let mut output_rx = bus.subscribe_output();
    let input_sender = bus.input_sender();
    let mut resize_rx = bus.subscribe_resize();

    if let Some((rows, cols)) = bus.current_size() {
        let msg = protocol::Message::resize(rows, cols).encode();
        ws.send(Message::binary(msg)).await?;
    }

    let last = last_seq.unwrap_or(0);
    let (full, replay_events) = bus.output_replay_from(last);

    let replay_mode = protocol::Message::replay_mode(full);
    ws.send(Message::binary(replay_mode.encode())).await?;

    let replay_mark = replay_events.last().map(|e| e.seq).unwrap_or(0);
    for event in &replay_events {
        let msg = protocol::Message::output(event.seq, event.data.clone()).encode();
        ws.send(Message::binary(msg)).await?;
    }

    let replay_end = protocol::Message {
        msg_type: protocol::MessageType::ReplayEnd,
        payload: Vec::new(),
    };
    ws.send(Message::binary(replay_end.encode())).await?;

    loop {
        match output_rx.try_recv() {
            Ok(event) if event.seq <= replay_mark => continue,
            Ok(event) => {
                let msg = protocol::Message::output(event.seq, event.data).encode();
                ws.send(Message::binary(msg)).await?;
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
        }
    }

    let (mut ws_sink, mut ws_stream) = ws.split();
    let output_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok(data) = output_rx.recv() => {
                    let msg = protocol::Message::output(data.seq, data.data).encode();
                    if ws_sink.send(Message::binary(msg)).await.is_err() {
                        break;
                    }
                }
                Ok((rows, cols)) = resize_rx.recv() => {
                    let msg = protocol::Message::resize(rows, cols).encode();
                    if ws_sink.send(Message::binary(msg)).await.is_err() {
                        break;
                    }
                }
                else => break,
            }
        }
    });

    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(Message::Binary(data)) => {
                if let Some(proto_msg) = protocol::Message::decode(&data) {
                    match proto_msg.msg_type {
                        protocol::MessageType::Input => {
                            let _ = input_sender.send(proto_msg.payload).await;
                        }
                        protocol::MessageType::Resize => {
                            if let Some((rows, cols)) = proto_msg.parse_resize() {
                                bus.send_resize(rows, cols);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(_)) => {}
            _ => {}
        }
    }

    output_handle.abort();
    Ok(())
}
