//! The world runner: the single logical writer of the city.
//!
//! It owns the `WorldState`, the `AssetCatalogue`, and the one `rusqlite`
//! connection. Network tasks never touch them — they only send
//! `WorldRequest`s into a bounded queue, which the runner drains in arrival
//! order. Every mutation follows the contract lifecycle:
//!
//! receipt lookup → validate → apply → ONE SQLite transaction →
//! broadcast events / direct result.
//!
//! M2 additions: player identity and presence. The runner owns the actor
//! registry: `identify` announces `player_joined`, input packets update the
//! broadcast presence map (fire-on-change, never persisted, never
//! receipted), and the ticker sweeps expired presences and lapsed player
//! leases. Commands are stamped with the server-resolved owner; the client
//! payload never names an actor.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use city_protocol as wire;
use city_simulation::{ActorId, AssetCatalogue, WorldState};
use tokio::sync::{mpsc, oneshot};

use crate::adapter;
use crate::hub::{ConnectionId, HubHandle, HubMsg};
use crate::persistence::{self, Store};

/// Capacity of the bounded ingress queue. When full, producers await
/// (backpressure) — no commands are dropped.
pub(crate) const QUEUE_CAPACITY: usize = 256;

/// Configurable TTLs (seconds), read from the environment at boot.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TtlConfig {
    /// Silence after which an actor's presence disappears (next tick).
    pub(crate) presence_secs: u64,
    /// Silence after which the player lease lapses and `player_left` fires.
    pub(crate) player_secs: u64,
}

/// One request into the single-writer queue.
pub(crate) enum WorldRequest {
    /// A parsed client command with its canonical receipt payload.
    Command {
        conn: ConnectionId,
        actor: ActorId,
        command: wire::Command,
        canonical: String,
    },
    /// Publish a validated recipe through the single writer.
    PublishAsset {
        recipe: wire::Recipe,
        reply: oneshot::Sender<Result<wire::AssetDefinition, Vec<String>>>,
    },
    /// Remove a definition; refused while placed buildings reference it.
    RemoveDefinition {
        def_id: u64,
        reply: oneshot::Sender<Result<(), String>>,
    },
    /// Full snapshot for a fresh `helloworld`.
    Snapshot {
        reply: oneshot::Sender<wire::WorldDto>,
    },
    /// The identify handshake completed: announce the join and answer with
    /// the world as of the join plus one `PresenceState` per actor already
    /// present. `None` on the reply means the runner is gone.
    Identified {
        conn: ConnectionId,
        actor: ActorId,
        reply: oneshot::Sender<Option<(wire::WorldDto, Vec<wire::PresenceState>)>>,
    },
    /// Cursor/drag input. Never receipted, never persisted.
    Input {
        actor: ActorId,
        input: wire::InputMessage,
    },
    /// The socket is gone (graceful close or dropped task).
    Disconnect { conn: ConnectionId, actor: ActorId },
}

/// What a handled request wants emitted afterwards.
struct Emissions {
    broadcast: Vec<wire::ServerMessage>,
    direct: Vec<(ConnectionId, wire::ServerMessage)>,
}

impl Emissions {
    fn none() -> Self {
        Self {
            broadcast: Vec::new(),
            direct: Vec::new(),
        }
    }

    fn to_issuer(conn: ConnectionId, result: wire::CommandResult) -> Self {
        Self {
            broadcast: Vec::new(),
            direct: vec![(conn, adapter::result_message(result))],
        }
    }
}

async fn emit(emissions: Emissions, hub: &HubHandle) {
    for msg in emissions.broadcast {
        send_broadcast(hub, msg).await;
    }
    for (conn, msg) in emissions.direct {
        send_direct(hub, conn, msg).await;
    }
}

/// Live per-actor bookkeeping. Presence state is `None` until the actor's
/// first input packet, so the broadcast map stays sparse.
struct PlayerRecord {
    label: String,
    /// The connection currently holding the identity, when online.
    conn: Option<ConnectionId>,
    /// Last broadcast presence state, `None` while silent.
    presence: Option<wire::PresenceState>,
    /// Last cursor activity (inputs only): arms the presence fade.
    presence_seen: Instant,
    /// Last activity (identify, input, or command): arms the player lease.
    player_seen: Instant,
}

pub(crate) async fn run(
    mut world: WorldState,
    mut catalogue: AssetCatalogue,
    store: Store,
    mut rx: mpsc::Receiver<WorldRequest>,
    hub: HubHandle,
    ttl: TtlConfig,
) {
    let mut players: HashMap<ActorId, PlayerRecord> = HashMap::new();
    // One-second sweep cadence; ticks emit only when something expires.
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            request = rx.recv() => {
                let Some(request) = request else { return; };
                let emissions = match request {
                    WorldRequest::Snapshot { reply } => {
                        let _ = reply.send(adapter::world_dto(&world, &catalogue));
                        Emissions::none()
                    }
                    WorldRequest::RemoveDefinition { def_id, reply } => {
                        remove_definition(&mut world, &mut catalogue, &store, def_id, reply)
                    }
                    WorldRequest::PublishAsset { recipe, reply } => {
                        publish_asset(&mut world, &mut catalogue, &store, recipe, reply)
                    }
                    WorldRequest::Identified { conn, actor, reply } => {
                        identified(
                            &world,
                            &catalogue,
                            &mut players,
                            &store,
                            conn,
                            actor,
                            reply,
                        )
                    }
                    WorldRequest::Input { actor, input } => {
                        input_packet(&mut players, actor, input)
                    }
                    WorldRequest::Disconnect { conn, actor } => {
                        disconnected(&mut players, conn, actor);
                        Emissions::none()
                    }
                    WorldRequest::Command {
                        conn,
                        actor,
                        command,
                        canonical,
                    } => {
                        touch_player(&mut players, &actor);
                        execute_command(
                            &mut world,
                            &mut catalogue,
                            &store,
                            conn,
                            actor,
                            command,
                            canonical,
                        )
                    }
                };
                emit(emissions, &hub).await;
            }
            _ = ticker.tick() => {
                emit(sweep_presences(&mut players, ttl), &hub).await;
            }
        }
    }
}

/// Deadline sweep: a lapsed player lease removes the record entirely and
/// broadcasts `player_left`; an offline actor past the presence TTL loses
/// its cursor row silently (the client drops the actor on `player_left` or
/// when presence stops). Runs on the ticker path only.
fn sweep_presences(
    players: &mut HashMap<ActorId, PlayerRecord>,
    ttl: TtlConfig,
) -> Emissions {
    let now = Instant::now();
    let lapsed: Vec<ActorId> = players
        .iter()
        .filter(|(_, record)| {
            now.duration_since(record.player_seen).as_secs() >= ttl.player_secs
        })
        .map(|(actor, _)| actor.clone())
        .collect();
    let mut broadcast = Vec::new();
    for actor in &lapsed {
        players.remove(actor);
        broadcast.push(adapter::event_message(wire::Event::PlayerLeft {
            actor: adapter::actor_id(actor),
        }));
    }
    for record in players.values_mut() {
        let silent = now.duration_since(record.presence_seen).as_secs() >= ttl.presence_secs;
        if record.conn.is_none() && silent {
            record.presence = None;
        }
    }
    Emissions {
        broadcast,
        direct: Vec::new(),
    }
}

/// Handle a completed identify: register the actor (persisting its label),
/// announce `player_joined`, and answer the socket with the world as of the
/// join plus one `PresenceState` per *other* actor already present (the
/// joiner has no cursor state yet; their own cursor is client-local).
///
/// Deliberately synchronous: borrows of `Store` (`!Sync`, rusqlite) held
/// across an await would make the runner future `!Send` and unspawnable.
#[allow(clippy::too_many_arguments)]
fn identified(
    world: &WorldState,
    catalogue: &AssetCatalogue,
    players: &mut HashMap<ActorId, PlayerRecord>,
    store: &Store,
    conn: ConnectionId,
    actor: ActorId,
    reply: oneshot::Sender<Option<(wire::WorldDto, Vec<wire::PresenceState>)>>,
) -> Emissions {
    let now = Instant::now();
    let label = match store.ensure_player(&actor) {
        Ok(label) => label,
        Err(err) => {
            eprintln!("runner: cannot persist player {}: {err}", actor.0);
            persistence::label_for(&actor)
        }
    };
    let record = players.entry(actor.clone()).or_insert(PlayerRecord {
        label: label.clone(),
        conn: None,
        presence: None,
        presence_seen: now,
        player_seen: now,
    });
    record.label = label.clone();
    record.conn = Some(conn);
    record.presence_seen = now;
    record.player_seen = now;

    let joined = adapter::event_message(wire::Event::PlayerJoined {
        actor: adapter::actor_id(&actor),
        label,
    });
    let presence = players
        .iter()
        .filter(|(other, _)| *other != &actor)
        .filter_map(|(_, record)| record.presence.clone())
        .collect();
    let _ = reply.send(Some((adapter::world_dto(world, catalogue), presence)));
    Emissions {
        broadcast: vec![joined],
        direct: Vec::new(),
    }
}

/// Cursor/drag input: refresh the lease, map to `PresenceState`, broadcast
/// only when the state actually changed. The LATEST input per actor wins:
/// the actor is dragging exactly when that input carries `def_id` (armed
/// hover counts as drag); `down`/`up` do not override this. Presence never
/// touches the store.
fn input_packet(
    players: &mut HashMap<ActorId, PlayerRecord>,
    actor: ActorId,
    input: wire::InputMessage,
) -> Emissions {
    let Some(record) = players.get_mut(&actor) else {
        // Unknown actor: identified after the lease lapsed, or never
        // identified. Nothing about this packet is world-visible.
        return Emissions::none();
    };
    let now = Instant::now();
    record.presence_seen = now;
    record.player_seen = now;
    let kind = if input.def_id.is_some() {
        wire::PresenceKind::Drag
    } else {
        wire::PresenceKind::Idle
    };
    let state = wire::PresenceState {
        actor: adapter::actor_id(&actor),
        label: record.label.clone(),
        cell: input.cell,
        kind,
        def_id: input.def_id.clone(),
        rotation: input.rotation,
    };
    if record.presence.as_ref() == Some(&state) {
        return Emissions::none();
    }
    record.presence = Some(state.clone());
    Emissions {
        broadcast: vec![adapter::event_message(wire::Event::Presence { presence: state })],
        direct: Vec::new(),
    }
}

/// Connection teardown: detach the socket. Per contract the presence row
/// fades on the next sweep after `presence_secs` of silence; the player
/// lease keeps ticking down for the remainder of `player_secs`.
fn disconnected(players: &mut HashMap<ActorId, PlayerRecord>, conn: ConnectionId, actor: ActorId) {
    if let Some(record) = players.get_mut(&actor) {
        if record.conn == Some(conn) {
            record.conn = None;
        }
    }
}

/// Every command re-arms the actor's lease (the presence clock is left
/// alone — commands are not cursor motion). Unknown actors gain no record:
/// the socket layer guarantees identify happened, so a missing record only
/// means the lease already lapsed.
fn touch_player(players: &mut HashMap<ActorId, PlayerRecord>, actor: &ActorId) {
    if let Some(record) = players.get_mut(actor) {
        record.player_seen = Instant::now();
    }
}

fn execute_command(
    world: &mut WorldState,
    catalogue: &mut AssetCatalogue,
    store: &Store,
    conn: ConnectionId,
    actor: ActorId,
    command: wire::Command,
    canonical: String,
) -> Emissions {
    let command_id = adapter::command_id(&command).to_owned();

    match store.receipt(&command_id) {
        Err(err) => {
            eprintln!("runner: receipt lookup failed for {command_id:?}: {err}");
            return Emissions::to_issuer(
                conn,
                adapter::bad_request(&command_id, "receipt lookup failed; command not applied"),
            );
        }
        Ok(Some((payload, outcome))) => {
            return if payload == canonical {
                // Idempotent resubmission: replay the stored outcome, apply nothing.
                Emissions::to_issuer(conn, outcome)
            } else {
                Emissions::to_issuer(
                    conn,
                    wire::CommandResult::Rejected {
                        command_id: command_id.clone(),
                        code: wire::RejectCode::DuplicateIdentityConflict,
                        message: format!(
                            "command_id {command_id:?} was already used with a different payload"
                        ),
                        conflicting_building_id: None,
                    },
                )
            };
        }
        Ok(None) => {}
    }

    let (outcome, events) = match command {
        wire::Command::PlaceBuilding {
            command_id,
            def_id,
            revision,
            cell,
            rotation,
        } => place_building(
            world,
            catalogue,
            store,
            &actor,
            &command_id,
            &def_id,
            revision,
            cell,
            rotation,
            &canonical,
        ),
        wire::Command::DemolishBuilding {
            command_id,
            building_id,
        } => demolish_building(
            world,
            store,
            &actor,
            &command_id,
            &building_id,
            &canonical,
        ),
        wire::Command::ClearBuildings { command_id } => {
            clear_buildings(world, store, &actor, &command_id, &canonical)
        }
    };

    Emissions {
        broadcast: events,
        direct: vec![(conn, adapter::result_message(outcome))],
    }
}

/// Lifecycle for one place command: receipt replay/conflict check, validate,
/// apply, one SQLite transaction, then the event broadcast. `actor` is always
/// server-resolved from the connection identity.
#[allow(clippy::too_many_arguments)]
fn place_building(
    world: &mut WorldState,
    catalogue: &mut AssetCatalogue,
    store: &Store,
    actor: &ActorId,
    command_id: &str,
    def_id: &str,
    revision: u32,
    cell: wire::Cell,
    rotation: u8,
    canonical: &str,
) -> (wire::CommandResult, Vec<wire::ServerMessage>) {
    let placement =
        match adapter::parse_placement(Some(actor.clone()), def_id, revision, cell, rotation) {
            Err(message) => {
                let outcome = receipt_rejection(
                    store,
                    command_id,
                    canonical,
                    adapter::bad_request(command_id, message),
                );
                return (outcome, Vec::new());
            }
            Ok(placement) => placement,
        };
    let Some(def) = catalogue.get(placement.def_id) else {
        let outcome = receipt_rejection(
            store,
            command_id,
            canonical,
            adapter::bad_request(
                command_id,
                format!("definition {def_id} not found (current revision 1)"),
            ),
        );
        return (outcome, Vec::new());
    };
    if let Err(reject) = city_simulation::validate_placement(world, def, &placement) {
        let outcome = receipt_rejection(
            store,
            command_id,
            canonical,
            adapter::reject_result(command_id, reject),
        );
        return (outcome, Vec::new());
    }

    // The id is allocated by the world before apply so the persistence
    // commit can cover counter, mutation, and receipt atomically.
    let id = world.next_building_id();
    let building = city_simulation::construction::apply_placement(world, def, &placement, id);
    let next_building_id = world.peek_next_building_id();
    let outcome = wire::CommandResult::Accepted {
        command_id: command_id.to_owned(),
        building_id: adapter::wire_id(id.0),
        owner: adapter::actor_id(actor),
    };
    if let Err(err) =
        store.commit_place(&building, next_building_id, command_id, canonical, &outcome)
    {
        // The in-memory state already moved; resynchronize from the
        // database so memory and disk agree before the next command.
        eprintln!("runner: commit_place failed for {command_id:?}: {err}");
        let (resynced_world, resynced_catalogue) = resync_from_disk(store);
        *world = resynced_world;
        *catalogue = resynced_catalogue;
        let outcome = adapter::bad_request(
            command_id,
            "persistence failed; world resynchronized from database",
        );
        return (outcome, Vec::new());
    }

    let event = adapter::event_message(wire::Event::BuildingPlaced {
        building: adapter::building_dto(&building, def),
    });
    (outcome, vec![event])
}

fn demolish_building(
    world: &mut WorldState,
    store: &Store,
    actor: &ActorId,
    command_id: &str,
    building_id: &str,
    canonical: &str,
) -> (wire::CommandResult, Vec<wire::ServerMessage>) {
    let id = match adapter::parse_wire_id(building_id) {
        Err(message) => {
            let outcome = receipt_rejection(
                store,
                command_id,
                canonical,
                adapter::bad_request(command_id, message),
            );
            return (outcome, Vec::new());
        }
        Ok(raw) => city_simulation::BuildingId(raw),
    };
    let _building = match city_simulation::construction::apply_demolition(world, id) {
        Ok(building) => building,
        Err(reject) => {
            let outcome = receipt_rejection(
                store,
                command_id,
                canonical,
                adapter::reject_result(command_id, reject),
            );
            return (outcome, Vec::new());
        }
    };

    let outcome = wire::CommandResult::Accepted {
        command_id: command_id.to_owned(),
        building_id: adapter::wire_id(id.0),
        owner: adapter::actor_id(actor),
    };
    if let Err(err) = store.commit_demolish(id, command_id, canonical, &outcome) {
        eprintln!("runner: commit_demolish failed for {command_id:?}: {err}");
        let (resynced_world, _resynced_catalogue) = resync_from_disk(store);
        *world = resynced_world;
        let outcome = adapter::bad_request(
            command_id,
            "persistence failed; world resynchronized from database",
        );
        return (outcome, Vec::new());
    }

    let event = adapter::event_message(wire::Event::BuildingDemolished {
        building_id: adapter::wire_id(id.0),
        actor: adapter::actor_id(actor),
    });
    (outcome, vec![event])
}

/// Lifecycle for `clear_buildings`: receipt replay/conflict check, bulk
/// remove (domain op), one SQLite transaction, then the event broadcast.
/// Removing zero buildings is a legal, receipted outcome.
fn clear_buildings(
    world: &mut WorldState,
    store: &Store,
    actor: &ActorId,
    command_id: &str,
    canonical: &str,
) -> (wire::CommandResult, Vec<wire::ServerMessage>) {
    let removed_list = city_simulation::construction::apply_clear(world);
    let removed = removed_list.len() as u64;

    let outcome = wire::CommandResult::ClearedBuildings {
        command_id: command_id.to_owned(),
        removed,
    };
    if let Err(err) = store.commit_clear(command_id, canonical, &outcome) {
        eprintln!("runner: commit_clear failed for {command_id:?}: {err}");
        let (resynced_world, _resynced_catalogue) = resync_from_disk(store);
        *world = resynced_world;
        let outcome = adapter::bad_request(
            command_id,
            "persistence failed; world resynchronized from database",
        );
        return (outcome, Vec::new());
    }

    let event = adapter::event_message(wire::Event::BuildingsCleared {
        removed,
        actor: adapter::actor_id(actor),
    });
    (outcome, vec![event])
}

/// Publish flow for `POST /api/assets`: validate → allocate id → one
/// transaction → broadcast. The HTTP reply carries the full definition.
fn publish_asset(
    world: &mut WorldState,
    catalogue: &mut AssetCatalogue,
    store: &Store,
    recipe: wire::Recipe,
    reply: oneshot::Sender<Result<wire::AssetDefinition, Vec<String>>>,
) -> Emissions {
    let approved = match city_simulation::assets::validate_recipe(&recipe) {
        Err(errors) => {
            let _ = reply.send(Err(errors));
            return Emissions::none();
        }
        Ok(approved) => approved,
    };
    let id = world.next_asset_id();
    match store.commit_asset(id, &recipe, &approved.derived) {
        Ok(()) => {
            catalogue.insert(id, approved.clone());
            let definition = adapter::definition_dto(id, &approved);
            let _ = reply.send(Ok(definition.clone()));
            Emissions {
                broadcast: vec![adapter::event_message(wire::Event::DefinitionPublished {
                    definition,
                })],
                direct: Vec::new(),
            }
        }
        Err(err) => {
            eprintln!("runner: commit_asset failed: {err}");
            let _ = reply.send(Err(vec![format!(
                "persistence failed; the definition was not saved: {err}"
            )]));
            Emissions::none()
        }
    }
}

fn receipt_rejection(
    store: &Store,
    command_id: &str,
    canonical: &str,
    outcome: wire::CommandResult,
) -> wire::CommandResult {
    if let Err(err) = store.commit_rejection(command_id, canonical, &outcome) {
        eprintln!("runner: could not persist rejection for {command_id:?}: {err}");
    }
    outcome
}

/// Removal flow for `DELETE /api/assets/{id}`: refuse while any placed
/// building still uses the definition, then one transaction → broadcast.
fn remove_definition(
    world: &mut WorldState,
    catalogue: &mut AssetCatalogue,
    store: &Store,
    def_id: u64,
    reply: oneshot::Sender<Result<(), String>>,
) -> Emissions {
    let id = city_simulation::AssetId(def_id);
    if catalogue.get(id).is_none() {
        let _ = reply.send(Err(format!("unknown definition {def_id}")));
        return Emissions::none();
    }
    let placed = world.buildings().any(|building| building.def_id == id);
    if placed {
        let _ = reply.send(Err(format!(
            "definition {def_id} is still used by placed buildings"
        )));
        return Emissions::none();
    }
    match store.commit_asset_removal(id) {
        Ok(()) => {
            catalogue.remove(id);
            let _ = reply.send(Ok(()));
            Emissions {
                broadcast: vec![adapter::event_message(wire::Event::DefinitionRemoved {
                    def_id: adapter::wire_id(def_id),
                })],
                direct: Vec::new(),
            }
        }
        Err(err) => {
            eprintln!("runner: commit_asset_removal failed: {err}");
            let _ = reply.send(Err(err));
            Emissions::none()
        }
    }
}

/// Rebuild world + catalogue from the database after a commit failure so
/// memory never diverges from the durable record.
fn resync_from_disk(store: &Store) -> (WorldState, AssetCatalogue) {
    match store.reload() {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("runner: resync failed: {err}");
            // Nothing better to serve; the error already surfaced.
            (WorldState::new(), AssetCatalogue::new())
        }
    }
}

async fn send_broadcast(hub: &HubHandle, msg: wire::ServerMessage) {
    if let Err(err) = hub.send(HubMsg::Broadcast(msg)).await {
        eprintln!("runner: hub unavailable: {err}");
    }
}

async fn send_direct(hub: &HubHandle, conn: ConnectionId, msg: wire::ServerMessage) {
    if let Err(err) = hub.send(HubMsg::Direct { conn, msg }).await {
        eprintln!("runner: hub unavailable: {err}");
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    //! Spawns hub + runner against a tempdir database and exposes registered
    //! connections so tests exercise the full pipeline.

    use std::time::Duration;

    use super::*;
    use crate::hub::OUTBOX_CAPACITY;
    use wire::Cell as WireCell;

    pub(crate) struct TestHarness {
        pub(crate) world_tx: mpsc::Sender<WorldRequest>,
        pub(crate) out_rx: mpsc::Receiver<wire::ServerMessage>,
        pub(crate) conn: ConnectionId,
    }

    /// Spawns hub + runner with fast presence TTLs (1s) so sweep behavior
    /// is testable, and registers connection 1 as a broadcast observer.
    pub(crate) async fn spawn_on_store(
        world: WorldState,
        catalogue: AssetCatalogue,
        store: Store,
    ) -> TestHarness {
        spawn_with_ttl(
            world,
            catalogue,
            store,
            TtlConfig {
                presence_secs: 1,
                player_secs: 3600,
            },
        )
        .await
    }

    pub(crate) async fn spawn_with_ttl(
        world: WorldState,
        catalogue: AssetCatalogue,
        store: Store,
        ttl: TtlConfig,
    ) -> TestHarness {
        let (world_tx, world_rx) = mpsc::channel(QUEUE_CAPACITY);
        let (hub_tx, hub_rx) = mpsc::channel(64);
        tokio::spawn(crate::hub::run(hub_rx));
        tokio::spawn(run(world, catalogue, store, world_rx, hub_tx.clone(), ttl));
        let (out_tx, out_rx) = mpsc::channel(OUTBOX_CAPACITY);
        hub_tx
            .send(HubMsg::Register {
                conn: 1,
                out: out_tx,
            })
            .await
            .unwrap();
        drop(hub_tx);
        TestHarness {
            world_tx,
            out_rx,
            conn: 1,
        }
    }

    pub(crate) fn place(command_id: &str, def_id: &str, x: i32, y: i32) -> wire::Command {
        wire::Command::PlaceBuilding {
            command_id: command_id.to_owned(),
            def_id: def_id.to_owned(),
            revision: 1,
            cell: WireCell { x, y },
            rotation: 0,
        }
    }

    pub(crate) fn demolish(command_id: &str, building_id: &str) -> wire::Command {
        wire::Command::DemolishBuilding {
            command_id: command_id.to_owned(),
            building_id: building_id.to_owned(),
        }
    }

    pub(crate) fn clear(command_id: &str) -> wire::Command {
        wire::Command::ClearBuildings {
            command_id: command_id.to_owned(),
        }
    }

    impl TestHarness {
        pub(crate) async fn send_command_as(&self, actor: &str, command: wire::Command) {
            let canonical = adapter::canonical_command(&command);
            self.world_tx
                .send(WorldRequest::Command {
                    conn: self.conn,
                    actor: ActorId(actor.to_owned()),
                    command,
                    canonical,
                })
                .await
                .unwrap();
        }

        pub(crate) async fn send_command(&self, command: wire::Command) {
            self.send_command_as(DEFAULT_ACTOR, command).await;
        }

        pub(crate) async fn snapshot(&mut self) -> wire::WorldDto {
            let (tx, rx) = oneshot::channel();
            self.world_tx
                .send(WorldRequest::Snapshot { reply: tx })
                .await
                .unwrap();
            rx.await.unwrap()
        }

        /// Next server message for the registered connection, with a
        /// generous timeout so a missing broadcast fails as a timeout, not
        /// a hang.
        pub(crate) async fn next_message(&mut self) -> wire::ServerMessage {
            tokio::time::timeout(Duration::from_secs(2), self.out_rx.recv())
                .await
                .expect("timed out waiting for a server message")
                .expect("connection registry dropped the mailbox")
        }

        pub(crate) async fn next_result(&mut self) -> wire::CommandResult {
            loop {
                match self.next_message().await {
                    wire::ServerMessage::CommandResult { result } => return result,
                    _ => continue,
                }
            }
        }

        pub(crate) async fn next_event(&mut self) -> wire::Event {
            loop {
                match self.next_message().await {
                    wire::ServerMessage::Event { event } => return event,
                    _ => continue,
                }
            }
        }

        /// Completes the identify handshake for the harness connection and
        /// swallows the `helloworld` reply.
        pub(crate) async fn identify(&mut self, actor: &str) -> wire::WorldDto {
            let (tx, rx) = oneshot::channel();
            self.world_tx
                .send(WorldRequest::Identified {
                    conn: self.conn,
                    actor: ActorId(actor.to_owned()),
                    reply: tx,
                })
                .await
                .unwrap();
            let (world, _presence) = rx.await.unwrap().expect("runner alive");
            world
        }

        pub(crate) async fn input(&self, kind: wire::InputKind, x: i32, y: i32, def_id: Option<&str>) {
            self.world_tx
                .send(WorldRequest::Input {
                    actor: ActorId(DEFAULT_ACTOR.to_owned()),
                    input: wire::InputMessage {
                        kind,
                        cell: WireCell { x, y },
                        def_id: def_id.map(str::to_owned),
                        rotation: def_id.map(|_| 0),
                    },
                })
                .await
                .unwrap();
        }

        /// Fails the test if any server message arrives within the window.
        pub(crate) async fn expect_quiet(&mut self, millis: u64) {
            let received =
                tokio::time::timeout(Duration::from_millis(millis), self.out_rx.recv()).await;
            assert!(received.is_err(), "expected silence, got a message");
        }
    }

    pub(crate) const DEFAULT_ACTOR: &str = "aaaaaaaa-0000-4000-8000-000000000000";

    pub(crate) async fn test_world() -> (TestHarness, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("city.db")).unwrap();
        store.seed().unwrap();
        let (world, catalogue) = store.reload().unwrap();
        let harness = spawn_on_store(world, catalogue, store).await;
        (harness, dir)
    }

    pub(crate) fn recipe(name: &str, archetype: wire::Archetype) -> wire::Recipe {
        wire::Recipe {
            name: name.to_owned(),
            archetype,
            footprint_w: 2,
            footprint_h: 2,
            floors: 2,
            base_color: "#b5654f".to_owned(),
            roof_color: "#3a3a3a".to_owned(),
            window_style: wire::WindowStyle::Grid,
            seed: 11,
            parts: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::*;
    use wire::Cell as WireCell;
    use super::*;
    use city_protocol::Archetype;
    use city_simulation::GRID_WIDTH;

    #[tokio::test]
    async fn place_validates_persists_and_broadcasts() {
        let (mut harness, _dir) = test_world().await;
        harness.send_command(place("c1", "1", 10, 12)).await;
        match harness.next_event().await {
            wire::Event::BuildingPlaced { building } => {
                assert_eq!(building.id, "1");
                assert_eq!(building.owner.as_deref(), Some(DEFAULT_ACTOR));
            }
            other => panic!("expected building_placed, got {other:?}"),
        }
        match harness.next_result().await {
            wire::CommandResult::Accepted {
                command_id,
                building_id,
                owner,
            } => {
                assert_eq!(command_id, "c1");
                assert_eq!(building_id, "1");
                assert_eq!(owner, DEFAULT_ACTOR, "owner is server-resolved");
            }
            other => panic!("expected acceptance, got {other:?}"),
        }
        assert_eq!(harness.snapshot().await.buildings.len(), 1);

        // Overlap is a rejection, receipted for replay.
        harness.send_command(place("c2", "1", 11, 12)).await;
        match harness.next_result().await {
            wire::CommandResult::Rejected {
                command_id,
                message,
                conflicting_building_id,
                ..
            } => {
                assert_eq!(command_id, "c2");
                assert_eq!(conflicting_building_id.as_deref(), Some("1"));
                assert!(message.contains("overlaps"), "message: {message}");
            }
            other => panic!("expected rejection, got {other:?}"),
        }

        // Resubmission replays the stored rejection (no re-validation, no
        // new event).
        harness.send_command(place("c2", "1", 11, 12)).await;
        match harness.next_result().await {
            wire::CommandResult::Rejected {
                command_id,
                conflicting_building_id,
                ..
            } => {
                assert_eq!(command_id, "c2");
                assert_eq!(conflicting_building_id.as_deref(), Some("1"));
            }
            other => panic!("expected replayed rejection, got {other:?}"),
        }
        assert_eq!(harness.snapshot().await.buildings.len(), 1);
    }

    #[tokio::test]
    async fn concurrent_commands_apply_exactly_once_in_queue_order() {
        let (mut harness, _dir) = test_world().await;
        // Three concurrent senders race for the queue; the runner allocates
        // ids sequentially in arrival order — no gaps, no duplicates.
        let mut senders = Vec::new();
        for index in 0..3u32 {
            let tx = harness.world_tx.clone();
            let conn = harness.conn;
            senders.push(tokio::spawn(async move {
                let command = place(&format!("c{index}"), "1", 10 + (index as i32) * 4, 10);
                let canonical = adapter::canonical_command(&command);
                tx.send(WorldRequest::Command {
                    conn,
                    actor: ActorId(DEFAULT_ACTOR.to_owned()),
                    command,
                    canonical,
                })
                .await
                .unwrap();
            }));
        }
        for handle in senders {
            handle.await.unwrap();
        }

        let mut ids = Vec::new();
        for _ in 0..3 {
            match harness.next_result().await {
                wire::CommandResult::Accepted { building_id, .. } => ids.push(building_id),
                other => panic!("expected acceptance, got {other:?}"),
            }
        }
        ids.sort();
        assert_eq!(ids, ["1", "2", "3"], "ids allocated 1..3 with no gaps");
        assert_eq!(harness.snapshot().await.buildings.len(), 3);
    }

    #[tokio::test]
    async fn demolish_frees_cells_and_broadcasts() {
        let (mut harness, _dir) = test_world().await;
        harness.send_command(place("c1", "1", 10, 12)).await;
        let _ = harness.next_event().await;
        let _ = harness.next_result().await;

        harness.send_command(demolish("d1", "1")).await;
        match harness.next_event().await {
            wire::Event::BuildingDemolished { building_id, actor } => {
                assert_eq!(building_id, "1");
                assert_eq!(actor, DEFAULT_ACTOR);
            }
            other => panic!("expected building_demolished, got {other:?}"),
        }
        match harness.next_result().await {
            wire::CommandResult::Accepted {
                command_id,
                building_id,
                owner,
            } => {
                assert_eq!(command_id, "d1");
                assert_eq!(building_id, "1");
                assert_eq!(owner, DEFAULT_ACTOR, "demolisher is recorded");
            }
            other => panic!("expected accepted, got {other:?}"),
        }
        assert!(harness.snapshot().await.buildings.is_empty());

        // Demolishing again → unknown_building, receipted.
        harness.send_command(demolish("d2", "1")).await;
        match harness.next_result().await {
            wire::CommandResult::Rejected { code, .. } => {
                assert_eq!(code, wire::RejectCode::UnknownBuilding);
            }
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn clear_buildings_broadcasts_count_and_replays_receipt() {
        let (mut harness, _dir) = test_world().await;
        harness.send_command(place("c1", "1", 10, 12)).await;
        let _ = harness.next_event().await;
        let _ = harness.next_result().await;
        harness.send_command(place("c2", "1", 20, 12)).await;
        let _ = harness.next_event().await;
        let _ = harness.next_result().await;

        harness.send_command(clear("x1")).await;
        match harness.next_event().await {
            wire::Event::BuildingsCleared { removed, actor } => {
                assert_eq!(removed, 2);
                assert_eq!(actor, DEFAULT_ACTOR);
            }
            other => panic!("expected buildings_cleared, got {other:?}"),
        }
        match harness.next_result().await {
            wire::CommandResult::ClearedBuildings { command_id, removed } => {
                assert_eq!(command_id, "x1");
                assert_eq!(removed, 2);
            }
            other => panic!("expected cleared_buildings result, got {other:?}"),
        }
        assert!(harness.snapshot().await.buildings.is_empty());

        // Replaying the command id returns the stored receipt — the SAME
        // removed count — and broadcasts nothing.
        harness.send_command(clear("x1")).await;
        match harness.next_result().await {
            wire::CommandResult::ClearedBuildings { command_id, removed } => {
                assert_eq!(command_id, "x1");
                assert_eq!(removed, 2, "receipt replays the first outcome");
            }
            other => panic!("expected replayed result, got {other:?}"),
        }
        assert!(harness.snapshot().await.buildings.is_empty());

        // Clearing an already-empty board is a legal zero outcome.
        harness.send_command(clear("x2")).await;
        match harness.next_event().await {
            wire::Event::BuildingsCleared { removed: 0, actor } => {
                assert_eq!(actor, DEFAULT_ACTOR);
            }
            other => panic!("expected zero-count buildings_cleared, got {other:?}"),
        }
        match harness.next_result().await {
            wire::CommandResult::ClearedBuildings { removed: 0, .. } => {}
            other => panic!("expected zero-count result, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn out_of_bounds_and_bad_wire_values_reject() {
        let (mut harness, _dir) = test_world().await;

        harness
            .send_command(place("c1", "1", GRID_WIDTH + 50, 0))
            .await;
        match harness.next_result().await {
            wire::CommandResult::Rejected {
                code,
                conflicting_building_id,
                ..
            } => {
                assert_eq!(code, wire::RejectCode::OutOfBounds);
                assert_eq!(conflicting_building_id, None);
            }
            other => panic!("expected rejection, got {other:?}"),
        }

        // Non-decimal def_id → bad_request, no event, receipted for replay.
        harness
            .send_command(place("c2", "not-a-number", 0, 0))
            .await;
        match harness.next_result().await {
            wire::CommandResult::Rejected { code, .. } => {
                assert_eq!(code, wire::RejectCode::BadRequest);
            }
            other => panic!("expected rejection, got {other:?}"),
        }
        harness
            .send_command(place("c2", "not-a-number", 0, 0))
            .await;
        match harness.next_result().await {
            wire::CommandResult::Rejected { code, .. } => {
                assert_eq!(code, wire::RejectCode::BadRequest, "replayed");
            }
            other => panic!("expected replayed rejection, got {other:?}"),
        }

        assert!(harness.snapshot().await.buildings.is_empty());
    }

    #[tokio::test]
    async fn malformed_command_enqueued_is_bad_request_with_state_untouched() {
        // The socket layer rejects unparseable JSON before it reaches the
        // queue; here the runner-side wire-value guards are checked: bad
        // values → bad_request, nothing applied, nothing broadcast.
        let (mut harness, _dir) = test_world().await;
        let before = harness.snapshot().await;

        let cases: Vec<wire::Command> = vec![
            wire::Command::PlaceBuilding {
                // non-decimal def_id
                command_id: "c1".to_owned(),
                def_id: "abc".to_owned(),
                revision: 1,
                cell: WireCell { x: 0, y: 0 },
                rotation: 0,
            },
            wire::Command::PlaceBuilding {
                // stale/unknown definition revision
                command_id: "c2".to_owned(),
                def_id: "1".to_owned(),
                revision: 2,
                cell: WireCell { x: 0, y: 0 },
                rotation: 0,
            },
            wire::Command::PlaceBuilding {
                // rotation out of range
                command_id: "c3".to_owned(),
                def_id: "1".to_owned(),
                revision: 1,
                cell: WireCell { x: 0, y: 0 },
                rotation: 9,
            },
        ];
        for command in cases {
            harness.send_command(command).await;
            match harness.next_result().await {
                wire::CommandResult::Rejected { code, .. } => {
                    assert_eq!(code, wire::RejectCode::BadRequest);
                }
                other => panic!("expected bad_request, got {other:?}"),
            }
        }

        let after = harness.snapshot().await;
        assert_eq!(before.buildings, after.buildings);
        assert_eq!(before.tick, after.tick);
        assert_eq!(before.definitions, after.definitions, "catalogue untouched");
    }

    #[tokio::test]
    async fn publish_asset_validates_derives_persists_and_broadcasts() {
        let (mut harness, dir) = test_world().await;

        // Invalid recipe: error list, nothing persisted, nothing broadcast.
        let mut invalid = recipe("Bad", Archetype::House);
        invalid.floors = 0;
        let (tx, rx) = oneshot::channel();
        harness
            .world_tx
            .send(WorldRequest::PublishAsset {
                recipe: invalid,
                reply: tx,
            })
            .await
            .unwrap();
        let errors = rx.await.unwrap().unwrap_err();
        assert!(!errors.is_empty());

        // Valid recipe: id 4 (after the 3 seeds), derived stats, broadcast.
        // Office: 2x2x2 = 8 cell·floors → capacity floor(8*0.30)=2, cost 8*260=2080.
        let good = recipe("Test Block", Archetype::Office);
        let (tx, rx) = oneshot::channel();
        harness
            .world_tx
            .send(WorldRequest::PublishAsset {
                recipe: good.clone(),
                reply: tx,
            })
            .await
            .unwrap();
        let definition = rx.await.unwrap().expect("published");
        assert_eq!(definition.id, "4");
        assert_eq!(definition.revision, 1);
        assert_eq!(definition.recipe.name, "Test Block");
        assert_eq!(definition.derived.capacity, 2);
        assert_eq!(definition.derived.cost_credits, 2080);
        match harness.next_event().await {
            wire::Event::DefinitionPublished { definition } => assert_eq!(definition.id, "4"),
            other => panic!("expected definition_published, got {other:?}"),
        }
        drop(harness); // close the runner before reopening the store

        let store = Store::open(&dir.path().join("city.db")).unwrap();
        let save = store.load().expect("reopened store must load");
        assert_eq!(save.assets.len(), 4);
        assert_eq!(save.assets[3].recipe.name, "Test Block");
        assert_eq!(save.next_asset_id, 5);
    }

    #[tokio::test]
    async fn persistence_roundtrip_place_reopen_same_buildings() {
        let (mut harness, dir) = test_world().await;
        let db_path = dir.path().join("city.db");
        harness.send_command(place("c1", "1", 10, 12)).await;
        let _ = harness.next_event().await;
        let _ = harness.next_result().await;
        harness.send_command(place("c2", "3", 60, 60)).await;
        let _ = harness.next_event().await;
        let _ = harness.next_result().await;
        harness.send_command(demolish("d1", "2")).await;
        let _ = harness.next_event().await;
        let _ = harness.next_result().await;
        let before = harness.snapshot().await;
        drop(harness); // close channels; runner task ends; dir stays alive

        // Boot path: load tables → from_save → same world.
        let store = Store::open(&db_path).unwrap();
        let (world, catalogue) = store.reload().unwrap();
        let after = adapter::world_dto(&world, &catalogue);
        assert_eq!(before, after, "reload must reproduce the exact world DTO");
        assert_eq!(after.buildings.len(), 1);
        assert_eq!(after.buildings[0].id, "1");
        assert_eq!(after.buildings[0].owner.as_deref(), Some(DEFAULT_ACTOR));
        assert_eq!(after.buildings[0].cell, WireCell { x: 10, y: 12 });
        assert_eq!(after.definitions.len(), 3);

        // Counters survived: the next placement continues the id sequence.
        let mut harness2 = spawn_on_store(world, catalogue, store).await;
        harness2.send_command(place("c9", "1", 0, 0)).await;
        match harness2.next_result().await {
            wire::CommandResult::Accepted { building_id, .. } => {
                assert_eq!(
                    building_id, "3",
                    "ids never rewind: next is 3 after building 2 was demolished"
                )
            }
            other => panic!("expected accepted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn hello_snapshot_shape_matches_contract() {
        let (mut harness, _dir) = test_world().await;
        let dto = harness.snapshot().await;
        assert_eq!(dto.grid.width, 96);
        assert_eq!(dto.grid.height, 96);
        assert_eq!(dto.tick, 0);
        assert!(dto.buildings.is_empty());
        assert_eq!(dto.definitions.len(), 3);
        // Definitions are sorted by id and carry decimal wire ids.
        let ids: Vec<&str> = dto.definitions.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["1", "2", "3"]);
        assert_eq!(dto.definitions[0].recipe.name, "Cedar Walk-up");
        assert_eq!(dto.definitions[1].recipe.name, "Harbor Point Office");
        assert_eq!(dto.definitions[2].recipe.name, "Meridian Tower");
        // Derived values come from the normative derivation table.
        assert_eq!(dto.definitions[0].derived.capacity, 2);
        assert_eq!(dto.definitions[0].derived.cost_credits, 2160);
        assert_eq!(
            dto.definitions[2].derived.capacity, 38,
            "tower: floor(96*0.40)"
        );
        assert_eq!(dto.definitions[2].derived.cost_credits, 32640);
    }

    // ---- M2 identity and presence ----

    #[tokio::test]
    async fn identify_announces_join_and_snapshots_other_presence() {
        let (mut harness, _dir) = test_world().await;

        // First actor identifies: join event, empty presence snapshot.
        let world = harness.identify(DEFAULT_ACTOR).await;
        assert!(world.buildings.is_empty());
        match harness.next_event().await {
            wire::Event::PlayerJoined { actor, label } => {
                assert_eq!(actor, DEFAULT_ACTOR);
                assert_eq!(label, "player-aaaa");
            }
            other => panic!("expected player_joined, got {other:?}"),
        }

        // Their first input produces a presence event; the label rides along.
        harness.input(wire::InputKind::Move, 3, 4, None).await;
        match harness.next_event().await {
            wire::Event::Presence { presence } => {
                assert_eq!(presence.actor, DEFAULT_ACTOR);
                assert_eq!(presence.cell, WireCell { x: 3, y: 4 });
                assert_eq!(presence.kind, wire::PresenceKind::Idle);
                assert_eq!(presence.def_id, None);
            }
            other => panic!("expected presence, got {other:?}"),
        }

        // A second connection's identify answers with the first actor's
        // presence and broadcasts a second join.
        let (tx, rx) = oneshot::channel();
        harness
            .world_tx
            .send(WorldRequest::Identified {
                conn: 2,
                actor: ActorId("bbbbbbbb-0000-4000-8000-000000000000".to_owned()),
                reply: tx,
            })
            .await
            .unwrap();
        let (_world, presence) = rx.await.unwrap().expect("runner alive");
        assert_eq!(presence.len(), 1, "only the first actor has state");
        assert_eq!(presence[0].actor, DEFAULT_ACTOR);
        match harness.next_event().await {
            wire::Event::PlayerJoined { actor, label } => {
                assert_eq!(actor, "bbbbbbbb-0000-4000-8000-000000000000");
                assert_eq!(label, "player-bbbb");
            }
            other => panic!("expected second player_joined, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn armed_input_is_drag_and_disarmed_is_idle() {
        let (mut harness, _dir) = test_world().await;
        harness.identify(DEFAULT_ACTOR).await;
        let _ = harness.next_event().await;

        // down with an armed definition → drag ghost.
        harness
            .input(wire::InputKind::Down, 5, 6, Some("2"))
            .await;
        match harness.next_event().await {
            wire::Event::Presence { presence } => {
                assert_eq!(presence.kind, wire::PresenceKind::Drag);
                assert_eq!(presence.def_id.as_deref(), Some("2"));
                assert_eq!(presence.rotation, Some(0));
            }
            other => panic!("expected presence, got {other:?}"),
        }

        // Armed hover (move with def_id) also counts as drag.
        harness
            .input(wire::InputKind::Move, 6, 6, Some("2"))
            .await;
        match harness.next_event().await {
            wire::Event::Presence { presence } => {
                assert_eq!(presence.kind, wire::PresenceKind::Drag);
                assert_eq!(presence.cell, WireCell { x: 6, y: 6 });
            }
            other => panic!("expected presence, got {other:?}"),
        }

        // up without a definition → back to idle.
        harness.input(wire::InputKind::Up, 6, 6, None).await;
        match harness.next_event().await {
            wire::Event::Presence { presence } => {
                assert_eq!(presence.kind, wire::PresenceKind::Idle);
            }
            other => panic!("expected presence, got {other:?}"),
        }

        // A move at the same cell with the same idle kind is suppressed.
        harness.input(wire::InputKind::Move, 6, 6, None).await;
        harness.expect_quiet(150).await;
    }

    #[tokio::test]
    async fn lease_expiry_broadcasts_player_left_and_presence_fades_offline() {
        // presence 1s, player 2s: identify, then go silent. The sweep must
        // fire player_left at ~2s; a faded presence emits nothing (drop the
        // cursor when the actor's events stop).
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("city.db")).unwrap();
        store.seed().unwrap();
        let (world, catalogue) = store.reload().unwrap();
        let mut harness = spawn_with_ttl(
            world,
            catalogue,
            store,
            TtlConfig {
                presence_secs: 1,
                player_secs: 2,
            },
        )
        .await;

        harness.identify(DEFAULT_ACTOR).await;
        match harness.next_event().await {
            wire::Event::PlayerJoined { .. } => {}
            other => panic!("expected player_joined, got {other:?}"),
        }
        // Put a cursor on the map so there is presence to fade.
        harness.input(wire::InputKind::Move, 1, 2, None).await;
        match harness.next_event().await {
            wire::Event::Presence { .. } => {}
            other => panic!("expected presence, got {other:?}"),
        }

        // The lease is 2s and the sweep cadence adds up to 1s of slack, so
        // wait well past both; every message before player_left is skipped.
        let left = loop {
            let message = tokio::time::timeout(Duration::from_secs(6), harness.out_rx.recv())
                .await
                .expect("player_left must arrive within the lease window")
                .expect("connection registry dropped the mailbox");
            match message {
                wire::ServerMessage::Event {
                    event: wire::Event::PlayerLeft { actor },
                } => break actor,
                wire::ServerMessage::Event { .. } => continue,
                other => panic!("unexpected message: {other:?}"),
            }
        };
        assert_eq!(left, DEFAULT_ACTOR);
    }

    #[tokio::test]
    async fn disconnect_then_command_keeps_world_consistent() {
        let (mut harness, _dir) = test_world().await;
        harness.identify(DEFAULT_ACTOR).await;
        let _ = harness.next_event().await;

        harness
            .world_tx
            .send(WorldRequest::Disconnect {
                conn: harness.conn,
                actor: ActorId(DEFAULT_ACTOR.to_owned()),
            })
            .await
            .unwrap();

        // A command from the (now disconnected but not yet swept) actor
        // still applies and re-arms the lease.
        harness.send_command(place("c1", "1", 10, 12)).await;
        match harness.next_result().await {
            wire::CommandResult::Accepted { owner, .. } => {
                assert_eq!(owner, DEFAULT_ACTOR);
            }
            other => panic!("expected accepted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn command_from_unknown_actor_still_executes() {
        let (mut harness, _dir) = test_world().await;
        // No identify ever happened (the socket layer would enforce it; the
        // runner is defensive): the command still applies, and no presence
        // machinery invents a record.
        harness.send_command(place("c1", "1", 0, 0)).await;
        match harness.next_result().await {
            wire::CommandResult::Accepted { owner, .. } => {
                assert_eq!(owner, DEFAULT_ACTOR);
            }
            other => panic!("expected accepted, got {other:?}"),
        }
        harness.expect_quiet(150).await;
    }

    #[tokio::test]
    async fn duplicate_command_id_with_different_payload_is_identity_conflict() {
        let (mut harness, _dir) = test_world().await;
        harness.send_command(place("c1", "1", 10, 12)).await;
        let _ = harness.next_event().await;
        let _ = harness.next_result().await;

        // Same id, different payload → identity conflict.
        harness.send_command(place("c1", "1", 20, 20)).await;
        match harness.next_result().await {
            wire::CommandResult::Rejected { code, .. } => {
                assert_eq!(code, wire::RejectCode::DuplicateIdentityConflict);
            }
            other => panic!("expected identity conflict, got {other:?}"),
        }
    }
}
