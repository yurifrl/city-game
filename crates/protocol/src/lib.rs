//! Wire DTOs shared by server and web client. Pure data: serde + ts-rs only.
//! The JSON shape defined here is normative (docs/contracts/multiplayer-stream.md).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Decimal string of a u64. JS numbers are unsafe above 2^53.
pub type WireId = String;

/// Actor identity: the UUID the seat obtained from `GET /api/seed`. A
/// reconnecting seat re-sends the same key, so this string is stable across
/// connections. Domain twin: `city_simulation::ActorId`.
pub type ActorId = String;

pub const TS_EXPORT_DIR: &str = "../../../apps/web/src/protocol/generated/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub struct Cell {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub enum Archetype {
    House,
    Shop,
    Office,
    Tower,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub enum WindowStyle {
    Grid,
    Stripes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub struct AssetPart {
    /// Open vocabulary from the Asset Studio ("box", "roof_gable", "cylinder",
    /// …). Renderers map unknown kinds to "box"; validation checks geometry
    /// and color, not the kind string.
    pub kind: String,
    /// Metres relative to the footprint's min corner, y up from the ground.
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
    pub h: f32,
    pub d: f32,
    /// `#rrggbb`.
    pub color: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub struct Recipe {
    pub name: String,
    pub archetype: Archetype,
    pub footprint_w: u32,
    pub footprint_h: u32,
    pub floors: u32,
    pub base_color: String,
    pub roof_color: String,
    pub window_style: WindowStyle,
    pub seed: u32,
    /// Authored geometry from the Asset Studio. Empty = the procedural
    /// single-mass recipe the client compiler derives from the fields above.
    #[serde(default)]
    pub parts: Vec<AssetPart>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub struct DerivedStats {
    pub capacity: u32,
    pub cost_credits: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub struct AssetDefinition {
    pub id: WireId,
    pub revision: u32,
    pub recipe: Recipe,
    pub derived: DerivedStats,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub struct BuildingDto {
    pub id: WireId,
    pub def_id: WireId,
    pub revision: u32,
    pub cell: Cell,
    pub rotation: u8,
    pub derived: DerivedStats,
    /// Owner actor, resolved server-side. Absent on the wire when the
    /// building predates multiplayer (restored from a v1 database).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<ActorId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub struct GridDto {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub struct WorldDto {
    pub grid: GridDto,
    pub tick: u64,
    pub buildings: Vec<BuildingDto>,
    pub definitions: Vec<AssetDefinition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    PlaceBuilding {
        command_id: String,
        def_id: WireId,
        revision: u32,
        cell: Cell,
        rotation: u8,
    },
    DemolishBuilding {
        command_id: String,
        building_id: WireId,
    },
    ClearBuildings {
        command_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// First frame on every fresh connection. `key` is the seat's
    /// `/api/seed` UUID and becomes the actor identity for the connection.
    Identify { key: ActorId },
    Command { command: Command },
    /// Pointer/input stream for presence. Never carries an actor: the
    /// server stamps the connection identity onto it.
    Input { input: InputMessage },
}

/// One pointer sample. `cell` is always present; `def_id`/`rotation` are
/// present exactly when the placement tool is armed (hover included).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub struct InputMessage {
    pub kind: InputKind,
    pub cell: Cell,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub def_id: Option<WireId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Move,
    Down,
    Up,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    BuildingPlaced { building: BuildingDto },
    BuildingDemolished { building_id: WireId, actor: ActorId },
    /// `removed` counts the buildings that actually went away; zero is a
    /// legal outcome (the board was already empty).
    BuildingsCleared { removed: u64, actor: ActorId },
    DefinitionPublished { definition: AssetDefinition },
    DefinitionRemoved { def_id: WireId },
    /// Broadcast whenever an actor's presence state changes (move cell,
    /// arm/disarm the placement tool).
    Presence { presence: PresenceState },
    PlayerJoined { actor: ActorId, label: String },
    PlayerLeft { actor: ActorId },
}

/// Whether the actor is merely hovering the world (`idle`) or holding the
/// placement tool armed with a def ghost (`drag`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub enum PresenceKind {
    Idle,
    Drag,
}

/// One actor's presence snapshot: labelled cursor at `cell`, drag ghost when
/// the placement tool is armed. Idle state omits the def fields entirely.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub struct PresenceState {
    pub actor: ActorId,
    pub label: String,
    pub cell: Cell,
    /// `idle | drag`; `drag` exactly when `def_id` is present.
    pub kind: PresenceKind,
    /// Ghost definition when armed; absent when idle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub def_id: Option<WireId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum CommandResult {
    Accepted {
        command_id: String,
        building_id: WireId,
        /// Server-resolved owner actor for the newly placed building.
        owner: ActorId,
    },
    /// Outcome of `clear_buildings`: `removed` is part of the receipt, so a
    /// replayed command returns exactly what the first application removed.
    ClearedBuildings {
        command_id: String,
        removed: u64,
    },
    Rejected {
        command_id: String,
        code: RejectCode,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        conflicting_building_id: Option<WireId>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(rename_all = "snake_case")]
pub enum RejectCode {
    Overlap,
    OutOfBounds,
    UnknownAsset,
    UnknownBuilding,
    DuplicateIdentityConflict,
    BadRequest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    /// First frame on a fresh connection, after the client's `identify`:
    /// confirms the actor identity without any world data.
    Hello { you: ActorId },
    /// Follows `hello` immediately with the full world snapshot.
    /// The wire tag is `helloworld` (one word), not snake_case's
    /// `hello_world`, so the variant name is overridden explicitly.
    #[serde(rename = "helloworld")]
    HelloWorld { world: WorldDto },
    Event { event: Event },
    CommandResult { result: CommandResult },
}
