//! Per-connection WebSocket task.
//!
//! Handshake per the contract: the client's first frame MUST be `identify`,
//! the server answers `hello {you}` and then `helloworld {world}` — no world
//! data precedes `hello`. Afterwards the socket selects between the outbound
//! mailbox (hub) and inbound frames. Parse failures become `bad_request`
//! results on this socket only — nothing is broadcast, nothing is enqueued,
//! and the socket stays open. Valid commands enter the bounded ingress queue
//! in arrival order; inputs update that connection's presence.

use axum::extract::ws::{Message, WebSocket};
use city_protocol::{ClientMessage, ServerMessage};
use tokio::sync::mpsc;

use city_protocol as wire;

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
    {
        eprintln!("ws: connection {conn} could not register; runner gone");
        return;
    }

    // Handshake: wait for the first `identify` frame, then answer hello +
    // helloworld. Frames before identify are rejected on this socket only.
    let actor = match handshake(&app, &mut socket, &out_tx, conn).await {
        Some(actor) => actor,
        None => {
            let _ = app.hub.send(HubMsg::Unregister { conn }).await;
            return;
        }
    };

    loop {
        // The select must be biased toward the inbound socket: presence
        // updates are high-frequency and must not queue behind the hub
        // mailbox drain. Both sides run to completion per iteration.
        enum Step {
            Outbound(ServerMessage),
            Inbound(Message),
            Gone,
        }
        let step = tokio::select! {
            out = out_rx.recv() => match out {
                Some(message) => Step::Outbound(message),
                // Runner/hub gone.
                None => Step::Gone,
            },
            frame = socket.recv() => match frame {
                Some(Ok(message)) => Step::Inbound(message),
                Some(Err(err)) => {
                    eprintln!("ws: connection {conn} protocol error: {err}");
                    Step::Gone
                }
                None => Step::Gone,
            },
        };
        match step {
            Step::Gone => break,
            Step::Outbound(message) => {
                // Hub mailboxes carry JSON-ready `ServerMessage`s; serialization
                // is infallible (all fields are wire types), a send failure
                // means the socket is dead and ends the session.
                let text = serde_json::to_string(&message)
                    .expect("ServerMessage serialization cannot fail");
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            Step::Inbound(message) => match message {
                Message::Text(text) => {
                    handle_text(&app, &out_tx, conn, &actor, &text).await;
                }
                Message::Close(_) => break,
                _ => {}
            },
        }
    }

    let _ = app.hub.send(HubMsg::Unregister { conn }).await;
    // The runner detaches the connection so the actor's presence row fades
    // on the next sweep after the presence TTL of silence.
    let _ = app
        .world_tx
        .send(WorldRequest::Disconnect { conn, actor })
        .await;
}

/// Waits for the client's first frame. `identify {key}` becomes the
/// connection's actor identity; anything else is rejected without closing.
/// On success answers `hello` then `helloworld` and returns the actor.
async fn handshake(
    app: &App,
    socket: &mut WebSocket,
    out_tx: &mpsc::Sender<ServerMessage>,
    conn: u64,
) -> Option<city_simulation::ActorId> {
    loop {
        let frame = match socket.recv().await {
            Some(Ok(message)) => message,
            Some(Err(err)) => {
                eprintln!("ws: connection {conn} protocol error: {err}");
                return None;
            }
            None => return None,
        };
        let Message::Text(text) = frame else {
            continue;
        };
        match adapter::parse_client_message(&text) {
            Ok(ClientMessage::Identify { key }) => {
                let actor = city_simulation::ActorId(key);
                let _ = out_tx
                    .send(ServerMessage::Hello {
                        you: adapter::actor_id(&actor),
                    })
                    .await;
                // The world owner answers with the snapshot as of the join
                // and one presence state per already-present actor; the join
                // event fans out to every connection as a side effect.
                let Some((world, presence)) = app.identify(conn, actor.clone()).await else {
                    eprintln!("ws: connection {conn} runner unavailable during identify");
                    return None;
                };
                let _ = out_tx
                    .send(ServerMessage::HelloWorld { world })
                    .await;
                for state in presence {
                    let _ = out_tx
                        .send(ServerMessage::Event {
                            event: wire::Event::Presence { presence: state },
                        })
                        .await;
                }
                return Some(actor);
            }
            Ok(_) => {
                let _ = out_tx
                    .try_send(malformed("the first frame must be {\"type\":\"identify\",\"key\":\"<uuid>\"}"));
            }
            Err(bad_request) => {
                let _ = out_tx.try_send(bad_request);
            }
        }
    }
}

/// Parses one inbound WS text frame. Malformed input → `bad_request` on this
/// socket only; commands → bounded queue (awaiting capacity keeps ordering);
/// inputs → presence updates on the runner.
async fn handle_text(
    app: &App,
    out_tx: &mpsc::Sender<ServerMessage>,
    conn: u64,
    actor: &city_simulation::ActorId,
    text: &str,
) {
    match adapter::parse_client_message(text) {
        Ok(ClientMessage::Command { command }) => {
            let canonical = adapter::canonical_command(&command);
            let _ = app
                .world_tx
                .send(WorldRequest::Command {
                    conn,
                    actor: actor.clone(),
                    command,
                    canonical,
                })
                .await;
        }
        Ok(ClientMessage::Input { input }) => {
            let _ = app
                .world_tx
                .send(WorldRequest::Input {
                    actor: actor.clone(),
                    input,
                })
                .await;
        }
        // A second identify is a protocol mistake; reject it on this socket.
        Ok(ClientMessage::Identify { .. }) => {
            let _ = out_tx.try_send(malformed("already identified; identify is the first frame only"));
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
