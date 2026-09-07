//! The world runner: the single logical writer of the city.
//!
//! It owns the `WorldState`, the `AssetCatalogue`, and the one `rusqlite`
//! connection. Network tasks never touch them — they only send
//! `WorldRequest`s into a bounded queue, which the runner drains in arrival
//! order. Every mutation follows the contract lifecycle:
//!
//! receipt lookup → validate → apply → ONE SQLite transaction →
//! broadcast events / reply to issuer.
//!
//! All work per request is synchronous (validation, the world mutation, the
//! SQLite commit); the resulting messages are emitted to the hub only after
//! the borrows end. That keeps the runner future `Send` (rusqlite
//! connections are `Send` but not `Sync`) and enforces commit-before-
//! broadcast by construction.
//!
//! SQLite work here is short, local (WAL), and strictly serialized behind
//! this task, so blocking calls are acceptable without a thread pool.

use city_protocol as wire;
use city_simulation::{AssetCatalogue, BuildingId, WorldState};
use tokio::sync::{mpsc, oneshot};

use crate::adapter;
use crate::hub::{ConnectionId, HubHandle, HubMsg};
use crate::persistence::Store;

/// Capacity of the bounded ingress queue. When full, producers await
/// (backpressure) — no commands are dropped.
pub(crate) const QUEUE_CAPACITY: usize = 256;

pub(crate) enum WorldRequest {
    /// A parsed client command with its canonical receipt payload.
    Command { conn: ConnectionId, command: wire::Command, canonical: String },
    /// Publish a validated recipe through the single writer.
    PublishAsset {
        recipe: wire::Recipe,
        reply: oneshot::Sender<Result<wire::AssetDefinition, Vec<String>>>,
    },
    /// Full world snapshot for `hello` and `GET /api/assets`.
    Snapshot { reply: oneshot::Sender<wire::WorldDto> },
}

/// Messages produced by one request: events for everyone, results for the
/// issuing connection only. Emitted strictly after the state change is
/// committed.
struct Emissions {
    broadcast: Vec<wire::ServerMessage>,
    direct: Vec<(ConnectionId, wire::ServerMessage)>,
}

impl Emissions {
    fn none() -> Emissions {
        Emissions { broadcast: Vec::new(), direct: Vec::new() }
    }

    fn to_issuer(conn: ConnectionId, result: wire::CommandResult) -> Emissions {
        Emissions { broadcast: Vec::new(), direct: vec![(conn, adapter::result_message(result))] }
    }
}

/// Runs until every `WorldRequest` sender is dropped.
pub(crate) async fn run(
    mut world: WorldState,
    mut catalogue: AssetCatalogue,
    store: Store,
    mut rx: mpsc::Receiver<WorldRequest>,
    hub: HubHandle,
) {
    while let Some(request) = rx.recv().await {
        let emissions = match request {
            WorldRequest::Snapshot { reply } => {
                let _ = reply.send(adapter::world_dto(&world, &catalogue));
                Emissions::none()
            }
            WorldRequest::PublishAsset { recipe, reply } => {
                publish_asset(&mut world, &mut catalogue, &store, recipe, reply)
            }
            WorldRequest::Command { conn, command, canonical } => {
                execute_command(&mut world, &mut catalogue, &store, conn, command, canonical)
            }
        };
        for msg in emissions.broadcast {
            send_broadcast(&hub, msg).await;
        }
        for (conn, msg) in emissions.direct {
            send_direct(&hub, conn, msg).await;
        }
    }
}

/// Lifecycle for one command: receipt replay/conflict check, validate,
/// apply, single transaction, then broadcast + reply. Rejected outcomes are
/// receipted too, so resubmissions replay the same verdict.
fn execute_command(
    world: &mut WorldState,
    catalogue: &mut AssetCatalogue,
    store: &Store,
    conn: ConnectionId,
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
        wire::Command::PlaceBuilding { command_id, def_id, revision, cell, rotation } => place_building(
            world, catalogue, store, &command_id, &def_id, revision, cell, rotation, &canonical,
        ),
        wire::Command::DemolishBuilding { command_id, building_id } => {
            demolish_building(world, catalogue, store, &command_id, &building_id, &canonical)
        }
    };

    Emissions {
        broadcast: events,
        direct: vec![(conn, adapter::result_message(outcome))],
    }
}

fn place_building(
    world: &mut WorldState,
    catalogue: &mut AssetCatalogue,
    store: &Store,
    command_id: &str,
    def_id: &str,
    revision: u32,
    cell: wire::Cell,
    rotation: u8,
    canonical: &str,
) -> (wire::CommandResult, Vec<wire::ServerMessage>) {
    let placement = match adapter::parse_placement(def_id, revision, cell, rotation) {
        Err(message) => {
            let outcome =
                receipt_rejection(store, command_id, canonical, adapter::bad_request(command_id, message));
            return (outcome, Vec::new());
        }
        Ok(placement) => placement,
    };
    let Some(def) = catalogue.get(placement.def_id) else {
        let outcome = receipt_rejection(
            store,
            command_id,
            canonical,
            adapter::reject_result(command_id, city_simulation::Reject::UnknownAsset(placement.def_id)),
        );
        return (outcome, Vec::new());
    };
    if let Err(reject) = city_simulation::validate_placement(world, def, &placement) {
        let outcome =
            receipt_rejection(store, command_id, canonical, adapter::reject_result(command_id, reject));
        return (outcome, Vec::new());
    }

    // The id is allocated by the world before apply so the persistence
    // commit can cover counter, mutation, and receipt atomically.
    let id = world.next_building_id();
    let building = city_simulation::apply_placement(world, def, &placement, id);
    let outcome = wire::CommandResult::Accepted {
        command_id: command_id.to_owned(),
        building_id: adapter::wire_id(id.0),
    };
    if let Err(err) =
        store.commit_place(&building, world.peek_next_building_id(), command_id, canonical, &outcome)
    {
        resync_from_disk(world, catalogue, store, &err);
        let outcome =
            adapter::bad_request(command_id, "persistence failed; world resynchronized from database");
        return (outcome, Vec::new());
    }

    let event = adapter::event_message(wire::Event::BuildingPlaced {
        building: adapter::building_dto(&building, def),
    });
    (outcome, vec![event])
}

fn demolish_building(
    world: &mut WorldState,
    catalogue: &mut AssetCatalogue,
    store: &Store,
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
                adapter::bad_request(command_id, format!("building_id invalid: {message}")),
            );
            return (outcome, Vec::new());
        }
        Ok(raw) => BuildingId(raw),
    };
    let _removed = match city_simulation::apply_demolition(world, id) {
        Err(reject) => {
            let outcome =
                receipt_rejection(store, command_id, canonical, adapter::reject_result(command_id, reject));
            return (outcome, Vec::new());
        }
        Ok(removed) => removed,
    };
    let outcome = wire::CommandResult::Accepted {
        command_id: command_id.to_owned(),
        building_id: adapter::wire_id(id.0),
    };
    if let Err(err) = store.commit_demolish(id, command_id, canonical, &outcome) {
        resync_from_disk(world, catalogue, store, &err);
        let outcome =
            adapter::bad_request(command_id, "persistence failed; world resynchronized from database");
        return (outcome, Vec::new());
    }

    let event = adapter::event_message(wire::Event::BuildingDemolished {
        building_id: adapter::wire_id(id.0),
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
    if let Err(err) = store.commit_asset(id, &recipe, &approved.derived) {
        resync_from_disk(world, catalogue, store, &err);
        let _ = reply.send(Err(vec![format!("persistence failed: {err}")]));
        return Emissions::none();
    }
    catalogue.insert(id, approved);
    let definition = adapter::definition_dto(id, catalogue.get(id).expect("just inserted"));
    let _ = reply.send(Ok(definition.clone()));
    Emissions {
        broadcast: vec![adapter::event_message(wire::Event::DefinitionPublished { definition })],
        direct: Vec::new(),
    }
}

/// Records the outcome of a rejected command so resubmissions replay it.
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

/// Resync path when a commit fails: the database is the source of truth, so
/// reload state from it. Only reachable on IO failures (disk full, db
/// corruption). If even the reload fails, stop the process — an
/// authoritative server must never run on unknown state.
fn resync_from_disk(world: &mut WorldState, catalogue: &mut AssetCatalogue, store: &Store, err: &str) {
    eprintln!("runner: commit failed: {err}; resynchronizing world from database");
    match store.reload() {
        Ok((loaded_world, loaded_catalogue)) => {
            *world = loaded_world;
            *catalogue = loaded_catalogue;
        }
        Err(reload_err) => {
            eprintln!("runner: fatal: reload failed: {reload_err}");
            std::process::exit(1);
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
    //! Spawns hub + runner against a tempdir database and exposes a
    //! registered connection so tests exercise the full pipeline.

    use std::time::Duration;

    use super::*;
    use crate::hub::OUTBOX_CAPACITY;

    pub(crate) struct TestHarness {
        pub(crate) world_tx: mpsc::Sender<WorldRequest>,
        pub(crate) hub: HubHandle,
        pub(crate) out_rx: mpsc::Receiver<wire::ServerMessage>,
        pub(crate) conn: ConnectionId,
    }

    impl TestHarness {
        /// Skips `Event` frames in the socket mailbox and returns the next
        /// `CommandResult`. Panics after 5s so tests fail instead of hanging.
        pub(crate) async fn next_result(&mut self) -> wire::CommandResult {
            loop {
                let msg = tokio::time::timeout(Duration::from_secs(5), self.out_rx.recv())
                    .await
                    .expect("timed out waiting for command result")
                    .expect("connection dropped before command result");
                if let wire::ServerMessage::CommandResult { result } = msg {
                    return result;
                }
            }
        }

        pub(crate) async fn next_event(&mut self) -> wire::Event {
            loop {
                let msg = tokio::time::timeout(Duration::from_secs(5), self.out_rx.recv())
                    .await
                    .expect("timed out waiting for event")
                    .expect("connection dropped before event");
                if let wire::ServerMessage::Event { event } = msg {
                    return event;
                }
            }
        }

        pub(crate) async fn send_command(&self, command: wire::Command) {
            let canonical = adapter::canonical_command(&command);
            self.world_tx
                .send(WorldRequest::Command { conn: self.conn, command, canonical })
                .await
                .expect("runner alive");
        }

        pub(crate) async fn snapshot(&self) -> wire::WorldDto {
            let (tx, rx) = oneshot::channel();
            self.world_tx
                .send(WorldRequest::Snapshot { reply: tx })
                .await
                .expect("runner alive");
            tokio::time::timeout(Duration::from_secs(5), rx)
                .await
                .expect("timed out waiting for snapshot")
                .expect("runner dropped reply")
        }
    }

    /// Fresh seeded store in a tempdir, hub + runner spawned, connection 1
    /// registered. The caller owns the dir (and thus the database lifetime).
    pub(crate) async fn test_world() -> (TestHarness, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("city.db");
        let store = Store::open(&db_path).expect("store opens");
        store.seed().expect("seed");
        let (world, catalogue) = store.reload().expect("reload");

        let (world_tx, world_rx) = mpsc::channel(QUEUE_CAPACITY);
        let (hub_tx, hub_rx) = mpsc::channel(64);
        tokio::spawn(crate::hub::run(hub_rx));
        tokio::spawn(run(world, catalogue, store, world_rx, hub_tx.clone()));

        let conn = 1;
        let (out_tx, out_rx) = mpsc::channel(OUTBOX_CAPACITY);
        hub_tx.send(HubMsg::Register { conn, out: out_tx }).await.expect("hub alive");

        (TestHarness { world_tx, hub: hub_tx, out_rx, conn }, dir)
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::*;
    use super::*;
    use city_protocol::{Archetype, Cell as WireCell, Recipe, WindowStyle};
    use city_simulation::GRID_WIDTH;

    /// Spawns a second runner on an already-opened store (for reload tests).
    async fn spawn_on_store(world: WorldState, catalogue: AssetCatalogue, store: Store) -> TestHarness {
        let (world_tx, world_rx) = mpsc::channel(QUEUE_CAPACITY);
        let (hub_tx, hub_rx) = mpsc::channel(64);
        tokio::spawn(crate::hub::run(hub_rx));
        tokio::spawn(run(world, catalogue, store, world_rx, hub_tx.clone()));
        let (out_tx, out_rx) = mpsc::channel(crate::hub::OUTBOX_CAPACITY);
        hub_tx.send(HubMsg::Register { conn: 1, out: out_tx }).await.unwrap();
        TestHarness { world_tx, hub: hub_tx, out_rx, conn: 1 }
    }

    fn place(command_id: &str, def_id: &str, x: i32, y: i32) -> wire::Command {
        wire::Command::PlaceBuilding {
            command_id: command_id.to_owned(),
            def_id: def_id.to_owned(),
            revision: 1,
            cell: WireCell { x, y },
            rotation: 0,
        }
    }

    fn demolish(command_id: &str, building_id: &str) -> wire::Command {
        wire::Command::DemolishBuilding {
            command_id: command_id.to_owned(),
            building_id: building_id.to_owned(),
        }
    }

    fn recipe(name: &str, archetype: Archetype) -> Recipe {
        Recipe {
            name: name.to_owned(),
            archetype,
            footprint_w: 2,
            footprint_h: 2,
            floors: 2,
            base_color: "#123456".to_owned(),
            roof_color: "#234567".to_owned(),
            window_style: WindowStyle::Grid,
            seed: 7,
        }
    }

    #[tokio::test]
    async fn places_building_and_broadcasts_event_then_result() {
        let (mut harness, _dir) = test_world().await;
        harness.send_command(place("c1", "1", 10, 12)).await;

        // Contract order: events first, then the issuer's result.
        match harness.next_event().await {
            wire::Event::BuildingPlaced { building } => {
                assert_eq!(building.id, "1");
                assert_eq!(building.def_id, "1");
                assert_eq!(building.cell, WireCell { x: 10, y: 12 });
                assert_eq!(building.derived.capacity, 2, "Cedar Walk-up: floor(18*0.15)");
                assert_eq!(building.derived.cost_credits, 2160);
            }
            other => panic!("expected building_placed, got {other:?}"),
        }
        match harness.next_result().await {
            wire::CommandResult::Accepted { command_id, building_id } => {
                assert_eq!(command_id, "c1");
                assert_eq!(building_id, "1");
            }
            other => panic!("expected accepted, got {other:?}"),
        }

        let dto = harness.snapshot().await;
        assert_eq!(dto.buildings.len(), 1);
        assert_eq!(dto.definitions.len(), 3, "three seeded definitions");
    }

    #[tokio::test]
    async fn identical_resubmission_replays_stored_outcome() {
        let (mut harness, _dir) = test_world().await;
        harness.send_command(place("c1", "1", 10, 12)).await;
        let first = harness.next_result().await;

        // Same command_id + identical payload: replayed outcome, no new
        // building, no new event.
        harness.send_command(place("c1", "1", 10, 12)).await;
        let replay = harness.next_result().await;
        assert_eq!(replay, first);
        assert_eq!(harness.snapshot().await.buildings.len(), 1, "replay must not re-apply");
    }

    #[tokio::test]
    async fn same_command_id_different_payload_is_duplicate_identity_conflict() {
        let (mut harness, _dir) = test_world().await;
        harness.send_command(place("c1", "1", 10, 12)).await;
        let _ = harness.next_result().await;

        // Same id, different cell → conflict, state untouched.
        harness.send_command(place("c1", "1", 40, 40)).await;
        match harness.next_result().await {
            wire::CommandResult::Rejected { command_id, code, message, .. } => {
                assert_eq!(command_id, "c1");
                assert_eq!(code, wire::RejectCode::DuplicateIdentityConflict);
                assert!(message.contains("different payload"), "message: {message}");
            }
            other => panic!("expected rejection, got {other:?}"),
        }
        assert_eq!(harness.snapshot().await.buildings.len(), 1);

        // Also across command kinds: a demolish reusing c1 conflicts too.
        harness.send_command(demolish("c1", "1")).await;
        match harness.next_result().await {
            wire::CommandResult::Rejected { code, .. } => {
                assert_eq!(code, wire::RejectCode::DuplicateIdentityConflict);
            }
            other => panic!("expected rejection, got {other:?}"),
        }
        assert_eq!(harness.snapshot().await.buildings.len(), 1);
    }

    #[tokio::test]
    async fn rejected_placement_is_receipted_and_replayed() {
        let (mut harness, _dir) = test_world().await;
        harness.send_command(place("c1", "1", 10, 12)).await;
        let _ = harness.next_result().await;

        // Overlap with building 1 → rejected with the conflicting id.
        harness.send_command(place("c2", "1", 11, 12)).await;
        match harness.next_result().await {
            wire::CommandResult::Rejected {
                command_id,
                code,
                conflicting_building_id,
                message,
                ..
            } => {
                assert_eq!(command_id, "c2");
                assert_eq!(code, wire::RejectCode::Overlap);
                assert_eq!(conflicting_building_id.as_deref(), Some("1"));
                assert!(message.contains("overlaps"), "message: {message}");
            }
            other => panic!("expected rejection, got {other:?}"),
        }

        // Resubmission replays the stored rejection (no re-validation, no
        // new event).
        harness.send_command(place("c2", "1", 11, 12)).await;
        match harness.next_result().await {
            wire::CommandResult::Rejected { command_id, conflicting_building_id, .. } => {
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
                tx.send(WorldRequest::Command { conn, command, canonical }).await.unwrap();
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
            wire::Event::BuildingDemolished { building_id } => assert_eq!(building_id, "1"),
            other => panic!("expected building_demolished, got {other:?}"),
        }
        match harness.next_result().await {
            wire::CommandResult::Accepted { command_id, building_id } => {
                assert_eq!(command_id, "d1");
                assert_eq!(building_id, "1");
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
    async fn out_of_bounds_and_bad_wire_values_reject() {
        let (mut harness, _dir) = test_world().await;

        harness.send_command(place("c1", "1", GRID_WIDTH + 50, 0)).await;
        match harness.next_result().await {
            wire::CommandResult::Rejected { code, conflicting_building_id, .. } => {
                assert_eq!(code, wire::RejectCode::OutOfBounds);
                assert_eq!(conflicting_building_id, None);
            }
            other => panic!("expected rejection, got {other:?}"),
        }

        // Non-decimal def_id → bad_request, no event, receipted for replay.
        harness.send_command(place("c2", "not-a-number", 0, 0)).await;
        match harness.next_result().await {
            wire::CommandResult::Rejected { code, .. } => {
                assert_eq!(code, wire::RejectCode::BadRequest);
            }
            other => panic!("expected rejection, got {other:?}"),
        }
        harness.send_command(place("c2", "not-a-number", 0, 0)).await;
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
            wire::Command::PlaceBuilding { // non-decimal def_id
                command_id: "c1".to_owned(),
                def_id: "abc".to_owned(),
                revision: 1,
                cell: WireCell { x: 0, y: 0 },
                rotation: 0,
            },
            wire::Command::PlaceBuilding { // stale/unknown definition revision
                command_id: "c2".to_owned(),
                def_id: "1".to_owned(),
                revision: 2,
                cell: WireCell { x: 0, y: 0 },
                rotation: 0,
            },
            wire::Command::PlaceBuilding { // rotation out of range
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
            .send(WorldRequest::PublishAsset { recipe: invalid, reply: tx })
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
            .send(WorldRequest::PublishAsset { recipe: good.clone(), reply: tx })
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
        assert_eq!(after.buildings[0].cell, WireCell { x: 10, y: 12 });
        assert_eq!(after.definitions.len(), 3);

        // Counters survived: the next placement continues the id sequence.
        let mut harness2 = spawn_on_store(world, catalogue, store).await;
        harness2.send_command(place("c9", "1", 0, 0)).await;
        match harness2.next_result().await {
            wire::CommandResult::Accepted { building_id, .. } => {
                assert_eq!(building_id, "3", "ids never rewind: next is 3 after building 2 was demolished")
            }
            other => panic!("expected accepted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn hello_snapshot_shape_matches_contract() {
        let (harness, _dir) = test_world().await;
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
        assert_eq!(dto.definitions[2].derived.capacity, 38, "tower: floor(96*0.40)");
        assert_eq!(dto.definitions[2].derived.cost_credits, 32640);
    }
}
