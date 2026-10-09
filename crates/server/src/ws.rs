//! Per-connection WebSocket task.
//!
//! One task per socket, selecting between the outbound mailbox (hub) and
//! inbound frames. Parse failures become `bad_request` results on this
//! socket only — nothing is broadcast, nothing is enqueued, and the socket
//! stays open. Valid commands enter the bounded ingress queue in arrival
//! order.

use axum::extract::ws::{Message, WebSocket};
use city_protocol::ServerMessage;
use tokio::sync::mpsc;

use crate::adapter;
use crate::http::App;
use crate::hub::{HubMsg, OUTBOX_CAPACITY};
use crate::runner::WorldRequest;

/// Serves one upgraded socket until it closes, the hub drops it (slow
/// consumer), or the runner goes away.
pub(crate) async fn run_connection(app: App, mut socket: WebSocket) {
    let conn = app
        .next_conn
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let (out_tx, mut out_rx) = mpsc::channel::<ServerMessage>(OUTBOX_CAPACITY);
    if app
        .hub
        .send(HubMsg::Register {
            conn,
            out: out_tx.clone(),
        })
        .await
        .is_err()
    {}

    // hello: actor + full world DTO from the single source of truth.
    let (tx, rx) = tokio::sync::oneshot::channel();
    if app
        .world_tx
        .send(WorldRequest::Snapshot { reply: tx })
        .await
        .is_err()
    {
        return;
    }
    match rx.await {
        Ok(world) => {
            let hello = ServerMessage::Hello {
                actor: adapter::OWNER_ACTOR.to_owned(),
                world,
            };
            if out_tx.send(hello).await.is_err() {
                return;
            }
        }
        Err(_) => return,
    }

    loop {
        tokio::select! {
            outbound = out_rx.recv() => match outbound {
                Some(msg) => {
                    let text = serde_json::to_string(&msg).expect("ServerMessage serialization cannot fail");
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                // The hub dropped us (slow consumer) or shut down: close.
                None => break,
            },
            inbound = socket.recv() => match inbound {
                Some(Ok(Message::Text(text))) => {
                    handle_text(&app, out_tx.clone(), conn, text.as_str()).await;
                }
                Some(Ok(Message::Binary(_))) => {
                    let _ = out_tx.try_send(malformed("binary frames are not supported"));
                }
                Some(Ok(Message::Ping(payload))) => {
                    let _ = socket.send(Message::Pong(payload)).await;
                }
                // Pong needs no action; Close ends the session.
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(Message::Pong(_))) => {}
                Some(Err(err)) => {
                    eprintln!("ws: connection {conn} protocol error: {err}");
                    break;
                }
            },
        }
    }

    let _ = app.hub.send(HubMsg::Unregister { conn }).await;
}

/// Parses one text frame. Malformed input → `bad_request` on this socket
/// only; valid commands → bounded queue (awaiting capacity keeps ordering).
async fn handle_text(app: &App, out_tx: mpsc::Sender<ServerMessage>, conn: u64, text: &str) {
    match adapter::parse_client_message(text) {
        Ok(city_protocol::ClientMessage::Command { command }) => {
            let canonical = adapter::canonical_command(&command);
            let _ = app
                .world_tx
                .send(WorldRequest::Command {
                    conn,
                    command,
                    canonical,
                })
                .await;
        }
        Err(bad_request) => {
            let _ = out_tx.try_send(bad_request);
        }
    }
}

fn malformed(message: impl Into<String>) -> ServerMessage {
    ServerMessage::CommandResult {
        result: adapter::bad_request("", message),
    }
}
