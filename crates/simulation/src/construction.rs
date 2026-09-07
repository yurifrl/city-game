//! Placement validation and application. Validation is pure; application is
//! called only by the world owner after validation, inside one atomic commit.

use crate::assets::ApprovedDefinition;
use crate::error::Reject;
use crate::id::BuildingId;
use crate::world::{in_bounds, Building, Cell, WorldState, GRID_HEIGHT, GRID_WIDTH};

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub def_id: crate::id::AssetId,
    pub revision: u32,
    pub origin: Cell,
    pub rotation: u8,
}

/// Footprint cells for `width × height` at `origin`; odd rotations swap the
/// dimensions (quarter turns clockwise, rotation ∈ 0..=3 — only the parity of
/// the value affects geometry). Returns cells in row-major order starting at
/// the minimum cell.
pub fn footprint(origin: Cell, width: u32, height: u32, rotation: u8) -> Vec<Cell> {
    let (w, h) = if rotation % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    };
    let mut cells = Vec::with_capacity((w * h) as usize);
    for dy in 0..h {
        for dx in 0..w {
            cells.push(Cell {
                x: origin.x + dx as i32,
                y: origin.y + dy as i32,
            });
        }
    }
    cells
}

/// Pure validation: bounds, occupancy, definition existence/revision match.
///
/// The caller resolves `def` from the catalogue before calling: an unknown
/// `def_id` never reaches this function (the boundary reports `unknown_asset`),
/// and a stale `placement.revision` is likewise a boundary check because
/// `ApprovedDefinition` carries no revision of its own — the domain catalogue
/// stores immutable recipes keyed by id.
pub fn validate_placement(
    world: &WorldState,
    def: &ApprovedDefinition,
    placement: &Placement,
) -> Result<(), Reject> {
    let cells = footprint(
        placement.origin,
        def.recipe.footprint_w,
        def.recipe.footprint_h,
        placement.rotation,
    );
    for cell in &cells {
        if !in_bounds(*cell) {
            return Err(Reject::OutOfBounds(GRID_WIDTH, GRID_HEIGHT));
        }
    }
    for cell in cells {
        if let Some(conflicting) = world.occupant_at(cell) {
            return Err(Reject::Overlap { conflicting });
        }
    }
    Ok(())
}

/// Applies an already-validated placement. `id` is allocated by the caller via
/// `WorldState::next_building_id` so the persistence commit is atomic.
pub fn apply_placement(
    world: &mut WorldState,
    def: &ApprovedDefinition,
    placement: &Placement,
    id: BuildingId,
) -> Building {
    let cells = footprint(
        placement.origin,
        def.recipe.footprint_w,
        def.recipe.footprint_h,
        placement.rotation,
    );
    let building = Building {
        id,
        def_id: placement.def_id,
        revision: placement.revision,
        origin: placement.origin,
        rotation: placement.rotation,
    };
    world.insert_building(building.clone(), cells);
    building
}

/// Removes a building and frees its cells.
pub fn apply_demolition(world: &mut WorldState, id: BuildingId) -> Result<Building, Reject> {
    world.remove_building(id).ok_or(Reject::UnknownBuilding(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetCatalogue;
    use crate::id::AssetId;
    use city_protocol::{Archetype, DerivedStats, Recipe, WindowStyle};

    fn def(w: u32, h: u32) -> ApprovedDefinition {
        ApprovedDefinition {
            recipe: Recipe {
                name: "Test Block".into(),
                archetype: Archetype::House,
                footprint_w: w,
                footprint_h: h,
                floors: 1,
                base_color: "#b5654f".into(),
                roof_color: "#3a3a3a".into(),
                window_style: WindowStyle::Grid,
                seed: 1,
            },
            derived: DerivedStats { capacity: 0, cost_credits: 0 },
        }
    }

    fn placement(origin: Cell, rotation: u8) -> Placement {
        Placement {
            def_id: AssetId(1),
            revision: 1,
            origin,
            rotation,
        }
    }

    #[test]
    fn footprint_is_row_major_and_odd_rotations_swap_dimensions() {
        let origin = Cell { x: 5, y: 7 };

        let rot0 = footprint(origin, 3, 2, 0);
        assert_eq!(
            rot0,
            vec![
                Cell { x: 5, y: 7 },
                Cell { x: 6, y: 7 },
                Cell { x: 7, y: 7 },
                Cell { x: 5, y: 8 },
                Cell { x: 6, y: 8 },
                Cell { x: 7, y: 8 },
            ]
        );
        // Rotation 2 is a half turn: same footprint box as rotation 0.
        assert_eq!(footprint(origin, 3, 2, 2), rot0);

        let rot1 = footprint(origin, 3, 2, 1);
        assert_eq!(rot1.len(), 6);
        assert_eq!(rot1[0], origin, "anchored at the minimum cell");
        assert_eq!(rot1[1], Cell { x: 6, y: 7 });
        assert_eq!(rot1[2], Cell { x: 5, y: 8 });
        assert_eq!(rot1.iter().map(|c| c.x).max(), Some(6), "width becomes 2");
        assert_eq!(rot1.iter().map(|c| c.y).max(), Some(9), "height becomes 3");

        // Rotation 3 is odd too: swapped like rotation 1.
        let rot3 = footprint(origin, 3, 2, 3);
        assert_eq!(rot3.iter().map(|c| c.x).max(), Some(6));
        assert_eq!(rot3.iter().map(|c| c.y).max(), Some(9));
    }

    #[test]
    fn placement_outside_the_grid_is_rejected() {
        let world = WorldState::new();
        let definition = def(3, 2);

        let past_east = placement(Cell { x: 95, y: 10 }, 0);
        assert_eq!(
            validate_placement(&world, &definition, &past_east),
            Err(Reject::OutOfBounds(GRID_WIDTH, GRID_HEIGHT))
        );

        let past_south = placement(Cell { x: 10, y: 95 }, 0);
        assert_eq!(
            validate_placement(&world, &definition, &past_south),
            Err(Reject::OutOfBounds(GRID_WIDTH, GRID_HEIGHT))
        );

        let negative = placement(Cell { x: 0, y: -1 }, 0);
        assert_eq!(
            validate_placement(&world, &definition, &negative),
            Err(Reject::OutOfBounds(GRID_WIDTH, GRID_HEIGHT))
        );

        // The last row and column themselves are still legal for a 1×1.
        let tiny = def(1, 1);
        assert_eq!(
            validate_placement(&world, &tiny, &placement(Cell { x: 95, y: 95 }, 0)),
            Ok(())
        );
    }

    #[test]
    fn overlap_rejection_carries_the_conflicting_building_id() {
        let mut world = WorldState::new();
        let definition = def(3, 2);
        let first = apply_placement(&mut world, &definition, &placement(Cell { x: 10, y: 10 }, 0), BuildingId(1));

        // 2×2 at (11,11) covers (11,11), (12,11), (11,12), (12,12) — all inside
        // the first building's 3×2 box, so the conflict id must be building 1.
        let overlapping = placement(Cell { x: 11, y: 11 }, 0);
        assert_eq!(
            validate_placement(&world, &definition, &overlapping),
            Err(Reject::Overlap { conflicting: BuildingId(1) })
        );

        // Touching but not overlapping is legal.
        let adjacent = placement(Cell { x: 13, y: 10 }, 0);
        assert_eq!(validate_placement(&world, &definition, &adjacent), Ok(()));

        let _ = first;
    }

    #[test]
    fn placement_needs_the_whole_footprint_free() {
        let mut world = WorldState::new();
        let definition = def(2, 2);
        apply_placement(&mut world, &definition, &placement(Cell { x: 10, y: 10 }, 0), BuildingId(1));

        // Only the far corner touches the existing building, but the whole
        // footprint must still be free.
        let corner_touch = placement(Cell { x: 9, y: 9 }, 0);
        assert_eq!(
            validate_placement(&world, &definition, &corner_touch),
            Err(Reject::Overlap { conflicting: BuildingId(1) })
        );
    }

    /// Property: after any sequence of placements and demolitions the dense
    /// cell index agrees exactly with the building records — every footprint
    /// cell of every building maps back to it, and no cell is claimed twice.
    #[test]
    fn occupant_index_always_agrees_with_buildings() {
        fn assert_index_agrees(world: &WorldState, catalogue: &AssetCatalogue) {
            let mut expected: BTreeMap<(i32, i32), BuildingId> = BTreeMap::new();
            for building in world.buildings() {
                let recipe = &catalogue.get(building.def_id).expect("known def").recipe;
                for cell in footprint(building.origin, recipe.footprint_w, recipe.footprint_h, building.rotation) {
                    let previous = expected.insert((cell.x, cell.y), building.id);
                    assert!(previous.is_none(), "two buildings share cell {cell:?}");
                }
            }
            for y in 0..GRID_HEIGHT {
                for x in 0..GRID_WIDTH {
                    let cell = Cell { x, y };
                    assert_eq!(
                        world.occupant_at(cell),
                        expected.get(&(x, y)).copied(),
                        "index mismatch at {cell:?}"
                    );
                }
            }
        }

        let mut world = WorldState::new();
        let mut catalogue = AssetCatalogue::new();
        catalogue.insert(AssetId(1), def(3, 2));
        let definition = catalogue.get(AssetId(1)).unwrap();

        apply_placement(&mut world, definition, &placement(Cell { x: 4, y: 4 }, 0), BuildingId(1));
        apply_placement(&mut world, definition, &placement(Cell { x: 10, y: 4 }, 1), BuildingId(2));
        apply_placement(&mut world, definition, &placement(Cell { x: 4, y: 8 }, 0), BuildingId(3));
        assert_index_agrees(&world, &catalogue);

        let demolished = apply_demolition(&mut world, BuildingId(2)).unwrap();
        assert_eq!(demolished.id, BuildingId(2));
        assert_index_agrees(&world, &catalogue);

        // The freed cells accept a new rotated placement.
        apply_placement(&mut world, definition, &placement(Cell { x: 10, y: 4 }, 3), BuildingId(4));
        assert_index_agrees(&world, &catalogue);

        // A placement overlapping building 1 is refused and not applied.
        assert_eq!(
            validate_placement(&world, definition, &placement(Cell { x: 5, y: 4 }, 0)),
            Err(Reject::Overlap { conflicting: BuildingId(1) })
        );

        apply_demolition(&mut world, BuildingId(1)).unwrap();
        apply_placement(&mut world, definition, &placement(Cell { x: 5, y: 4 }, 0), BuildingId(5));
        assert_index_agrees(&world, &catalogue);

        // Demolishing an unknown building is an explicit error.
        assert_eq!(
            apply_demolition(&mut world, BuildingId(99)),
            Err(Reject::UnknownBuilding(BuildingId(99)))
        );
        assert_index_agrees(&world, &catalogue);
    }

    use std::collections::BTreeMap;

    #[test]
    fn applied_buildings_carry_the_placement_data() {
        let mut world = WorldState::new();
        let definition = def(3, 2);
        let building = apply_placement(&mut world, &definition, &placement(Cell { x: 4, y: 5 }, 1), BuildingId(9));
        assert_eq!(
            building,
            Building {
                id: BuildingId(9),
                def_id: AssetId(1),
                revision: 1,
                origin: Cell { x: 4, y: 5 },
                rotation: 1,
            }
        );
        assert_eq!(world.building(BuildingId(9)), Some(&building));
    }
}
