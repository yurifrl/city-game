//! Wire DTOs shared by server and web client. Pure data: serde + ts-rs only.
//! The JSON shape defined here is normative (docs/contracts/milestone-1.md).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Decimal string of a u64. JS numbers are unsafe above 2^53.
pub type WireId = String;

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Command { command: Command },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    BuildingPlaced { building: BuildingDto },
    BuildingDemolished { building_id: WireId },
    DefinitionPublished { definition: AssetDefinition },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../apps/web/src/protocol/generated/")]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum CommandResult {
    Accepted {
        command_id: String,
        building_id: WireId,
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
    Hello {
        actor: String,
        world: WorldDto,
    },
    Event {
        event: Event,
    },
    CommandResult {
        result: CommandResult,
    },
}
