//! End to end: the real server binary, a real GPU render, a viewer that only
//! sends raw input and receives frames, and a second raw game-wire client
//! that observes what the world actually committed.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

type Socket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const STEP: Duration = Duration::from_secs(20);
const FRAME_W: usize = 1280;
const FRAME_H: usize = 720;

struct Server {
    child: std::process::Child,
    addr: String,
    _db: tempfile::TempDir,
}

impl Server {
    async fn boot() -> Server {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let addr = format!("127.0.0.1:{port}");
        let db = tempfile::tempdir().unwrap();
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_city-server"))
            .env("CITY_BIND", &addr)
            .env("CITY_DB", db.path().join("city.db"))
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .expect("spawn city-server");
        let mut server = Server { child, addr, _db: db };
        for _ in 0..100 {
            if tokio::net::TcpStream::connect(&server.addr).await.is_ok() {
                return server;
            }
            if let Some(status) = server.child.try_wait().unwrap() {
                panic!("city-server exited during boot: {status}");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("city-server never listened on {}", server.addr);
    }

    async fn connect(&self, path: &str) -> Socket {
        let (socket, _) = tokio_tungstenite::connect_async(format!("ws://{}{path}", self.addr))
            .await
            .expect("ws connect");
        socket
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Everything a viewer receives.
enum Seen {
    Frame { seq: u64, rgb: Vec<u8> },
    Caption(String),
    Error(String),
}

async fn next_seen(viewer: &mut Socket) -> Seen {
    loop {
        let message = tokio::time::timeout(STEP, viewer.next())
            .await
            .expect("viewer went silent")
            .expect("viewer socket closed")
            .expect("viewer socket error");
        match message {
            Message::Binary(bytes) => {
                let meta_len = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;
                let meta: Value = serde_json::from_slice(&bytes[4..4 + meta_len]).unwrap();
                assert_eq!(meta["format"], "jpeg");
                assert_eq!(
                    (meta["w"].as_u64(), meta["h"].as_u64()),
                    (Some(FRAME_W as u64), Some(FRAME_H as u64))
                );
                let jpeg = &bytes[4 + meta_len..];
                assert_eq!(&jpeg[0..2], &[0xff, 0xd8], "not a JPEG");
                let rgb = jpeg_decoder::Decoder::new(jpeg).decode().expect("decodable JPEG");
                return Seen::Frame {
                    seq: meta["seq"].as_u64().unwrap(),
                    rgb,
                };
            }
            Message::Text(text) => {
                let value: Value = serde_json::from_str(&text).unwrap();
                match value["type"].as_str() {
                    Some("caption") => {
                        return Seen::Caption(value["text"].as_str().unwrap().to_owned())
                    }
                    Some("error") => {
                        return Seen::Error(value["message"].as_str().unwrap().to_owned())
                    }
                    other => panic!("unexpected viewer text message {other:?}"),
                }
            }
            _ => {}
        }
    }
}

async fn next_frame(viewer: &mut Socket) -> (u64, Vec<u8>) {
    loop {
        match next_seen(viewer).await {
            Seen::Frame { seq, rgb } => return (seq, rgb),
            Seen::Caption(_) => {}
            Seen::Error(message) => panic!("viewer error: {message}"),
        }
    }
}

async fn caption_containing(viewer: &mut Socket, needle: &str) -> String {
    loop {
        match next_seen(viewer).await {
            Seen::Caption(text) if text.contains(needle) => return text,
            Seen::Error(message) => panic!("viewer error: {message}"),
            _ => {}
        }
    }
}

fn centre_pixel(rgb: &[u8]) -> [u8; 3] {
    let i = (FRAME_H / 2 * FRAME_W + FRAME_W / 2) * 3;
    [rgb[i], rgb[i + 1], rgb[i + 2]]
}

async fn send(viewer: &mut Socket, value: Value) {
    viewer
        .send(Message::Text(value.to_string().into()))
        .await
        .unwrap();
}

/// Disarms the tool and waits until the viewer has seen the disarmed state:
/// the caption is sent before the frame of the same tick, so every frame
/// received afterwards is rendered without the placement ghost.
async fn disarm(viewer: &mut Socket) {
    send(viewer, json!({"type":"key","code":"Escape"})).await;
    loop {
        match next_seen(viewer).await {
            Seen::Caption(text) if text.starts_with("Nothing armed") => return,
            Seen::Error(message) => panic!("viewer error: {message}"),
            _ => {}
        }
    }
}

async fn click_centre(viewer: &mut Socket) {
    for action in ["move", "down", "up"] {
        send(
            viewer,
            json!({"type":"pointer","action":action,"x":640.0,"y":360.0,"button":0}),
        )
        .await;
    }
}

/// Reads game-wire messages until `want` returns Some.
async fn wire_event<T>(observer: &mut Socket, mut want: impl FnMut(&Value) -> Option<T>) -> T {
    loop {
        let message = tokio::time::timeout(STEP, observer.next())
            .await
            .expect("game wire went silent")
            .expect("game wire closed")
            .expect("game wire error");
        if let Message::Text(text) = message {
            if let Some(found) = want(&serde_json::from_str::<Value>(&text).unwrap()) {
                return found;
            }
        }
    }
}

#[tokio::test]
async fn a_viewer_places_and_demolishes_a_building_with_only_raw_input() {
    let server = Server::boot().await;

    // A raw game-wire observer: sees only what the world commits.
    let mut observer = server.connect("/ws").await;
    send(&mut observer, json!({"type":"identify","key":"observer"})).await;

    let mut viewer = server.connect("/stream?seed=viewer-a").await;
    let (first_seq, empty) = next_frame(&mut viewer).await;
    let ground = centre_pixel(&empty);
    assert!(
        ground[1] > ground[0] && ground[1] > ground[2],
        "empty city centre {ground:?} should be green ground"
    );

    // Arm the first stock definition, hover the centre, click: raw input only.
    send(&mut viewer, json!({"type":"key","code":"Digit1"})).await;
    caption_containing(&mut viewer, "Armed:").await;
    click_centre(&mut viewer).await;

    // The world committed it, owned by the viewer's server-resolved identity.
    let (building_id, cell) = wire_event(&mut observer, |m| {
        let event = m.get("event")?;
        (event["type"] == "building_placed").then(|| {
            assert_eq!(event["building"]["owner"], "viewer-a");
            (
                event["building"]["id"].as_str().unwrap().to_owned(),
                (
                    event["building"]["cell"]["x"].as_i64().unwrap(),
                    event["building"]["cell"]["y"].as_i64().unwrap(),
                ),
            )
        })
    })
    .await;
    assert!(
        (46..=49).contains(&cell.0) && (46..=49).contains(&cell.1),
        "placed at {cell:?}, expected near the grid centre"
    );

    // With the ghost gone, the next frame shows the building where there was
    // ground.
    disarm(&mut viewer).await;
    let (placed_seq, placed) = next_frame(&mut viewer).await;
    assert!(placed_seq > first_seq);
    assert_ne!(centre_pixel(&placed), ground, "building not drawn");

    // Placing on the same spot is rejected and the viewer is told why.
    send(&mut viewer, json!({"type":"key","code":"Digit1"})).await;
    caption_containing(&mut viewer, "Armed:").await;
    click_centre(&mut viewer).await;
    let caption = caption_containing(&mut viewer, "Rejected:").await;
    assert!(
        caption.to_lowercase().contains("overlap") || caption.to_lowercase().contains("occupied"),
        "caption: {caption}"
    );

    // Demolish through the same raw input path.
    disarm(&mut viewer).await;
    send(&mut viewer, json!({"type":"key","code":"KeyX"})).await;
    let removed = wire_event(&mut observer, |m| {
        let event = m.get("event")?;
        (event["type"] == "building_demolished")
            .then(|| event["building_id"].as_str().unwrap().to_owned())
    })
    .await;
    assert_eq!(removed, building_id);
    let after = loop {
        let (seq, rgb) = next_frame(&mut viewer).await;
        if seq > placed_seq && centre_pixel(&rgb) == ground {
            break rgb;
        }
    };
    assert_eq!(centre_pixel(&after), ground);
}

#[tokio::test]
async fn an_idle_viewer_costs_no_frames_and_malformed_input_is_reported_not_dropped() {
    let server = Server::boot().await;
    let mut viewer = server.connect("/stream?seed=idle").await;
    let (first, _) = next_frame(&mut viewer).await;

    send(&mut viewer, json!({"type":"bogus"})).await;
    let caption = caption_containing(&mut viewer, "Unreadable input").await;
    assert!(
        caption.contains("bogus") || caption.contains("unknown variant"),
        "caption: {caption}"
    );

    // Nothing changed in the picture, so no further frame may arrive.
    let mut frames_while_idle = 0;
    let _ = tokio::time::timeout(Duration::from_millis(600), async {
        loop {
            if let Seen::Frame { seq, .. } = next_seen(&mut viewer).await {
                assert!(seq > first);
                frames_while_idle += 1;
            }
        }
    })
    .await;
    assert_eq!(frames_while_idle, 0, "idle seat kept rendering");
}
