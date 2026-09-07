//! Placement validation and application. Validation is pure; application is
//! called only by the world owner after validation, inside one atomic commit.

use crate::assets::ApprovedDefinition;
use crate::error::Reject;
use crate::id::BuildingId;
use crate::world::{Building, Cell, WorldState, GRID_HEIGHT, GRID_WIDTH};

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub def_id: crate::id::AssetId,
    pub revision: u32,
    pub origin: Cell,
    pub rotation: u8,
}

/// Footprint cells for `width × height` at `origin`; odd rotations swap the
/// dimensions. Returns cells in row-major order.
pub fn footprint(origin: Cell, width: u32, height: u32, rotation: u8) -> Vec<Cell> {
    todo!("m1-01")
}

/// Pure validation: bounds, occupancy, definition existence/revision match.
pub fn validate_placement(
    world: &WorldState,
    def: &ApprovedDefinition,
    placement: &Placement,
) -> Result<(), Reject> {
    todo!("m1-01")
}

/// Applies an already-validated placement. `id` is allocated by the caller via
/// `WorldState::next_building_id` so the persistence commit is atomic.
pub fn apply_placement(
    world: &mut WorldState,
    def: &ApprovedDefinition,
    placement: &Placement,
    id: BuildingId,
) -> Building {
    todo!("m1-01")
}

/// Removes a building and frees its cells.
pub fn apply_demolition(world: &mut WorldState, id: BuildingId) -> Result<Building, Reject> {
    let _ = (GRID_WIDTH, GRID_HEIGHT);
    todo!("m1-01")
}
