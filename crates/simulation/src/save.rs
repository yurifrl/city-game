//! Durable save records. The server maps these to SQLite; the domain owns the
//! meaning. Stable ids and counters must survive a roundtrip exactly.

use crate::assets::{ApprovedDefinition, AssetCatalogue};
use crate::world::{Cell, WorldState};
use city_protocol::{DerivedStats, Recipe};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SaveBuilding {
    pub id: u64,
    pub def_id: u64,
    pub revision: u32,
    pub cell: Cell,
    pub rotation: u8,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SaveAsset {
    pub id: u64,
    pub recipe: Recipe,
    pub derived: DerivedStats,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WorldSave {
    pub tick: u64,
    pub next_building_id: u64,
    pub next_asset_id: u64,
    pub buildings: Vec<SaveBuilding>,
    pub assets: Vec<SaveAsset>,
}

pub fn to_save(world: &WorldState, catalogue: &AssetCatalogue) -> WorldSave {
    todo!("m1-01")
}

/// Rebuilds world + catalogue from durable records. Malformed records (missing
/// asset, duplicate cells, rotation ≥ 4) are errors, never silent defaults.
pub fn from_save(save: &WorldSave) -> Result<(WorldState, AssetCatalogue), String> {
    todo!("m1-01")
}

impl From<&ApprovedDefinition> for (Recipe, DerivedStats) {
    fn from(def: &ApprovedDefinition) -> Self {
        (def.recipe.clone(), def.derived)
    }
}
