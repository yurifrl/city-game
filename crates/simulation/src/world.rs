//! World state, grid geometry, and building records.

use std::collections::{BTreeMap, HashMap};

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

/// Grid membership test for a single cell.
pub(crate) fn in_bounds(cell: Cell) -> bool {
    cell.x >= 0 && cell.y >= 0 && cell.x < GRID_WIDTH && cell.y < GRID_HEIGHT
}

/// The single mutable world. One logical writer (the server's world runner).
///
/// Building records live in a dense id-ordered map. Occupancy is indexed by a
/// dense row-major cell grid (`occupancy[y * GRID_WIDTH + x]`) with a reverse
/// index from building id to its occupied cells. Both sides of the index are
/// maintained only by [`WorldState::insert_building`] and
/// [`WorldState::remove_building`], so `occupant_at` always agrees with
/// `buildings()`.
#[derive(Debug)]
pub struct WorldState {
    tick: u64,
    buildings: BTreeMap<BuildingId, Building>,
    occupancy: Vec<Option<BuildingId>>,
    occupied_cells: HashMap<BuildingId, Vec<Cell>>,
    next_building_id: u64,
    next_asset_id: u64,
}

impl WorldState {
    pub fn new() -> Self {
        Self::restored(0, 1, 1)
    }

    /// Builds a world with restored persistence state (tick and id counters).
    /// The counters must be strictly ahead of every id restored from the same
    /// records; `save::from_save` enforces that before calling this.
    pub(crate) fn restored(tick: u64, next_building_id: u64, next_asset_id: u64) -> Self {
        WorldState {
            tick,
            buildings: BTreeMap::new(),
            occupancy: vec![None; (GRID_WIDTH * GRID_HEIGHT) as usize],
            occupied_cells: HashMap::new(),
            next_building_id,
            next_asset_id,
        }
    }

    /// Flat row-major slot for `cell`, or `None` when outside the grid.
    fn slot(cell: Cell) -> Option<usize> {
        if !in_bounds(cell) {
            return None;
        }
        Some((cell.y * GRID_WIDTH + cell.x) as usize)
    }

    pub fn tick_count(&self) -> u64 {
        self.tick
    }

    /// Advances the persisted tick. M1 has no autonomous systems; keep this
    /// explicit so later phases extend it rather than invent a second clock.
    pub fn advance_tick(&mut self) {
        self.tick += 1;
    }

    pub fn building(&self, id: BuildingId) -> Option<&Building> {
        self.buildings.get(&id)
    }

    /// All buildings in ascending id order.
    pub fn buildings(&self) -> impl Iterator<Item = &Building> + '_ {
        self.buildings.values()
    }

    /// Building occupying `cell`, if any. Must always agree with `buildings()`.
    /// Cells outside the grid are never occupied.
    pub fn occupant_at(&self, cell: Cell) -> Option<BuildingId> {
        let slot = Self::slot(cell)?;
        self.occupancy[slot]
    }

    /// Stores a building and updates the cell index. Caller has validated:
    /// every `footprint_cells` entry must be inside the grid and free, and
    /// `building.id` must be unused. The footprint comes from the building's
    /// definition, which lives in the asset catalogue rather than the record,
    /// so the caller that resolved the definition supplies the cells.
    ///
    /// # Panics
    /// On out-of-bounds or occupied cells, or a duplicate id. Those are caller
    /// contract violations; the validated command path never produces them.
    pub fn insert_building(&mut self, building: Building, footprint_cells: Vec<Cell>) {
        assert!(
            !self.buildings.contains_key(&building.id),
            "duplicate building id {:?}",
            building.id
        );
        for &cell in &footprint_cells {
            let slot = Self::slot(cell).unwrap_or_else(|| {
                panic!(
                    "footprint cell {cell:?} of building {:?} is outside the grid",
                    building.id
                )
            });
            assert!(
                self.occupancy[slot].is_none(),
                "footprint cell {cell:?} of building {:?} is already occupied",
                building.id
            );
        }
        for &cell in &footprint_cells {
            let slot = Self::slot(cell).expect("bounds re-checked above");
            self.occupancy[slot] = Some(building.id);
        }
        self.occupied_cells.insert(building.id, footprint_cells);
        self.buildings.insert(building.id, building);
    }

    pub fn remove_building(&mut self, id: BuildingId) -> Option<Building> {
        let building = self.buildings.remove(&id)?;
        for cell in self
            .occupied_cells
            .remove(&id)
            .expect("reverse index has cells for every stored building")
        {
            let slot = Self::slot(cell).expect("stored buildings only cover in-bounds cells");
            if self.occupancy[slot] == Some(id) {
                self.occupancy[slot] = None;
            }
        }
        Some(building)
    }

    /// Allocates the next durable building identity.
    pub fn next_building_id(&mut self) -> BuildingId {
        let id = BuildingId(self.next_building_id);
        self.next_building_id += 1;
        id
    }

    /// Allocates the next durable asset identity.
    pub fn next_asset_id(&mut self) -> AssetId {
        let id = AssetId(self.next_asset_id);
        self.next_asset_id += 1;
        id
    }

    /// Next ids without allocating (save/reload continuity checks).
    pub fn peek_next_building_id(&self) -> u64 {
        self.next_building_id
    }

    pub fn peek_next_asset_id(&self) -> u64 {
        self.next_asset_id
    }
}

impl Default for WorldState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::construction::footprint;

    #[test]
    fn allocates_durable_ids_in_sequence_from_one() {
        let mut world = WorldState::new();
        assert_eq!(world.peek_next_building_id(), 1);
        assert_eq!(world.peek_next_asset_id(), 1);
        assert_eq!(world.next_building_id(), BuildingId(1));
        assert_eq!(world.next_building_id(), BuildingId(2));
        assert_eq!(world.next_asset_id(), AssetId(1));
        assert_eq!(world.peek_next_building_id(), 3);
        assert_eq!(world.peek_next_asset_id(), 2);
    }

    #[test]
    fn tick_counter_advances_from_zero() {
        let mut world = WorldState::new();
        assert_eq!(world.tick_count(), 0);
        world.advance_tick();
        world.advance_tick();
        assert_eq!(world.tick_count(), 2);
    }

    #[test]
    fn insert_and_remove_keep_index_and_storage_in_agreement() {
        let mut world = WorldState::new();
        let origin = Cell { x: 10, y: 12 };
        let cells = footprint(origin, 3, 2, 0);
        world.insert_building(
            Building {
                id: BuildingId(1),
                def_id: AssetId(1),
                revision: 1,
                origin,
                rotation: 0,
            },
            cells,
        );

        assert_eq!(
            world.building(BuildingId(1)).map(|b| b.origin),
            Some(origin)
        );
        assert_eq!(world.buildings().count(), 1);
        assert_eq!(world.occupant_at(origin), Some(BuildingId(1)));
        assert_eq!(
            world.occupant_at(Cell { x: 12, y: 13 }),
            Some(BuildingId(1))
        );
        // Just outside the 3-wide footprint at x = 10..=12.
        assert_eq!(world.occupant_at(Cell { x: 13, y: 12 }), None);

        assert_eq!(
            world.remove_building(BuildingId(1)).map(|b| b.id),
            Some(BuildingId(1))
        );
        assert_eq!(world.occupant_at(origin), None);
        assert_eq!(world.occupant_at(Cell { x: 12, y: 13 }), None);
        assert_eq!(world.buildings().next(), None);
        assert_eq!(world.remove_building(BuildingId(1)), None);
    }

    #[test]
    fn occupant_outside_the_grid_is_never_some() {
        let world = WorldState::new();
        assert_eq!(world.occupant_at(Cell { x: -1, y: 0 }), None);
        assert_eq!(
            world.occupant_at(Cell {
                x: 0,
                y: GRID_HEIGHT
            }),
            None
        );
        assert_eq!(
            world.occupant_at(Cell {
                x: GRID_WIDTH,
                y: 0
            }),
            None
        );
    }

    #[test]
    #[should_panic(expected = "already occupied")]
    fn insert_building_refuses_occupied_cells() {
        let mut world = WorldState::new();
        let cells = footprint(Cell { x: 0, y: 0 }, 2, 1, 0);
        let make = |id| Building {
            id,
            def_id: AssetId(1),
            revision: 1,
            origin: Cell { x: 0, y: 0 },
            rotation: 0,
        };
        world.insert_building(make(BuildingId(1)), cells.clone());
        world.insert_building(make(BuildingId(2)), cells);
    }

    #[test]
    #[should_panic(expected = "duplicate building id")]
    fn insert_building_refuses_duplicate_ids() {
        let mut world = WorldState::new();
        let first = footprint(Cell { x: 0, y: 0 }, 1, 1, 0);
        let second = footprint(Cell { x: 5, y: 5 }, 1, 1, 0);
        let make = |origin| Building {
            id: BuildingId(1),
            def_id: AssetId(1),
            revision: 1,
            origin,
            rotation: 0,
        };
        world.insert_building(make(Cell { x: 0, y: 0 }), first);
        world.insert_building(make(Cell { x: 5, y: 5 }), second);
    }
}
