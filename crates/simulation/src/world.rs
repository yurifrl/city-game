//! World state, grid geometry, and building records.

use crate::id::{AssetId, BuildingId};

pub const GRID_WIDTH: i32 = 96;
pub const GRID_HEIGHT: i32 = 96;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Cell {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Building {
    pub id: BuildingId,
    pub def_id: AssetId,
    pub revision: u32,
    /// Minimum (top-left) footprint cell.
    pub origin: Cell,
    pub rotation: u8,
}

/// The single mutable world. One logical writer (the server's world runner).
pub struct WorldState {
    // m1-01 owns the internal representation (dense map + cell index).
}

impl WorldState {
    pub fn new() -> Self {
        todo!("m1-01")
    }

    pub fn tick_count(&self) -> u64 {
        todo!("m1-01")
    }

    /// Advances the persisted tick. M1 has no autonomous systems; keep this
    /// explicit so later phases extend it rather than invent a second clock.
    pub fn advance_tick(&mut self) {
        todo!("m1-01")
    }

    pub fn building(&self, id: BuildingId) -> Option<&Building> {
        todo!("m1-01")
    }

    pub fn buildings(&self) -> impl Iterator<Item = &Building> + '_ {
        std::iter::from_fn(|| todo!("m1-01"))
    }

    /// Building occupying `cell`, if any. Must always agree with `buildings()`.
    pub fn occupant_at(&self, cell: Cell) -> Option<BuildingId> {
        todo!("m1-01")
    }

    /// Stores a building and updates the cell index. Caller has validated.
    pub fn insert_building(&mut self, building: Building) {
        todo!("m1-01")
    }

    pub fn remove_building(&mut self, id: BuildingId) -> Option<Building> {
        todo!("m1-01")
    }

    /// Allocates the next durable building identity.
    pub fn next_building_id(&mut self) -> BuildingId {
        todo!("m1-01")
    }

    /// Allocates the next durable asset identity.
    pub fn next_asset_id(&mut self) -> AssetId {
        todo!("m1-01")
    }

    /// Next ids without allocating (save/reload continuity checks).
    pub fn peek_next_building_id(&self) -> u64 {
        todo!("m1-01")
    }

    pub fn peek_next_asset_id(&self) -> u64 {
        todo!("m1-01")
    }
}

impl Default for WorldState {
    fn default() -> Self {
        Self::new()
    }
}
