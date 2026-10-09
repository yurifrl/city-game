//! Connection registry and fanout. The world runner produces messages; the
//! hub routes them: events to every registered socket, command results to
//! the issuing socket only.
//!
//! Per-socket mailboxes are bounded. A consumer that falls behind (mailbox
//! full) is dropped from the registry, which closes its socket on the next
//! `recv` — slow consumers never block the world.

use std::collections::HashMap;

use city_protocol::ServerMessage;
use tokio::sync::mpsc;

/// Capacity of each connection's outbound mailbox.
pub(crate) const OUTBOX_CAPACITY: usize = 64;

pub(crate) type ConnectionId = u64;

pub(crate) enum HubMsg {
    Register {
        conn: ConnectionId,
        out: mpsc::Sender<ServerMessage>,
    },
    Unregister {
        conn: ConnectionId,
    },
    /// Delivered to every registered connection. Delivery order per socket
    /// equals world commit order (single runner feeds the hub in order).
    Broadcast(ServerMessage),
    /// Delivered to one connection only (command results).
    Direct {
        conn: ConnectionId,
        msg: ServerMessage,
    },
}

pub(crate) type HubHandle = mpsc::Sender<HubMsg>;

/// Runs until the handle (and all clones) are dropped.
pub(crate) async fn run(mut rx: mpsc::Receiver<HubMsg>) {
    let mut connections: HashMap<ConnectionId, mpsc::Sender<ServerMessage>> = HashMap::new();
    while let Some(msg) = rx.recv().await {
        match msg {
            HubMsg::Register { conn, out } => {
                connections.insert(conn, out);
            }
            HubMsg::Unregister { conn } => {
                connections.remove(&conn);
            }
            HubMsg::Broadcast(msg) => {
                // try_send: a full mailbox means the consumer is too slow to
                // keep up with world events; drop it (its socket closes).
                connections.retain(|conn, out| match out.try_send(msg.clone()) {
                    Ok(()) => true,
                    Err(err) => {
                        eprintln!("hub: dropping connection {conn}: {err}");
                        false
                    }
                });
            }
            HubMsg::Direct { conn, msg } => match connections.get(&conn) {
                Some(out) => {
                    if out.try_send(msg.clone()).is_err() {
                        connections.remove(&conn);
                    }
                }
                None => eprintln!("hub: direct message for unknown connection {conn}"),
            },
        }
    }
}
