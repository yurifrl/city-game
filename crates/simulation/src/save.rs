//! Durable save records. The server maps these to SQLite; the domain owns the
//! meaning. Stable ids and counters must survive a roundtrip exactly.

use crate::assets::{ApprovedDefinition, AssetCatalogue};
use crate::construction::footprint;
use crate::id::{AssetId, BuildingId};
use crate::world::{in_bounds, Building, Cell, WorldState};
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
    WorldSave {
        tick: world.tick_count(),
        next_building_id: world.peek_next_building_id(),
        next_asset_id: world.peek_next_asset_id(),
        buildings: world
            .buildings()
            .map(|building| SaveBuilding {
                id: building.id.0,
                def_id: building.def_id.0,
                revision: building.revision,
                cell: building.origin,
                rotation: building.rotation,
            })
            .collect(),
        assets: catalogue
            .iter()
            .map(|(id, def)| SaveAsset {
                id: id.0,
                recipe: def.recipe.clone(),
                derived: def.derived,
            })
            .collect(),
    }
}

/// Rebuilds world + catalogue from durable records. Malformed records (unknown
/// asset id referenced by a building, duplicate ids, duplicate or out-of-bounds
/// cells, rotation ≥ 4, id counters not ahead of the restored ids) are errors,
/// never silent defaults.
pub fn from_save(save: &WorldSave) -> Result<(WorldState, AssetCatalogue), String> {
    let mut catalogue = AssetCatalogue::new();
    for asset in &save.assets {
        let id = AssetId(asset.id);
        if catalogue.get(id).is_some() {
            return Err(format!("duplicate asset id {}", asset.id));
        }
        catalogue.insert(
            id,
            ApprovedDefinition {
                recipe: asset.recipe.clone(),
                derived: asset.derived,
            },
        );
    }

    // Counters must allocate ids no saved record already uses, or a later
    // placement would mint a duplicate durable identity.
    if let Some(max) = save.buildings.iter().map(|b| b.id).max() {
        if save.next_building_id <= max {
            return Err(format!(
                "next_building_id {} is not ahead of restored building id {}",
                save.next_building_id, max
            ));
        }
    }
    if let Some(max) = save.assets.iter().map(|a| a.id).max() {
        if save.next_asset_id <= max {
            return Err(format!(
                "next_asset_id {} is not ahead of restored asset id {}",
                save.next_asset_id, max
            ));
        }
    }

    let mut world = WorldState::restored(save.tick, save.next_building_id, save.next_asset_id);
    for record in &save.buildings {
        let id = BuildingId(record.id);
        if world.building(id).is_some() {
            return Err(format!("duplicate building id {}", record.id));
        }
        if record.rotation > 3 {
            return Err(format!(
                "building {} has rotation {} outside 0..=3",
                record.id, record.rotation
            ));
        }
        let def = catalogue.get(AssetId(record.def_id)).ok_or_else(|| {
            format!(
                "building {} references unknown asset {}",
                record.id, record.def_id
            )
        })?;
        let cells = footprint(
            record.cell,
            def.recipe.footprint_w,
            def.recipe.footprint_h,
            record.rotation,
        );
        for cell in &cells {
            if !in_bounds(*cell) {
                return Err(format!(
                    "building {} has footprint cell ({}, {}) outside the grid",
                    record.id, cell.x, cell.y
                ));
            }
            if let Some(occupant) = world.occupant_at(*cell) {
                return Err(format!(
                    "building {} occupies cell ({}, {}) already held by building {:?}",
                    record.id, cell.x, cell.y, occupant
                ));
            }
        }
        world.insert_building(
            Building {
                id,
                def_id: AssetId(record.def_id),
                revision: record.revision,
                origin: record.cell,
                rotation: record.rotation,
            },
            cells,
        );
    }
    Ok((world, catalogue))
}

impl From<&ApprovedDefinition> for (Recipe, DerivedStats) {
    fn from(def: &ApprovedDefinition) -> Self {
        (def.recipe.clone(), def.derived)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::validate_recipe;
    use crate::construction::{apply_placement, Placement};
    use city_protocol::{Archetype, WindowStyle};

    fn cedar() -> Recipe {
        Recipe {
            name: "Cedar Walk-up".into(),
            archetype: Archetype::House,
            footprint_w: 3,
            footprint_h: 2,
            floors: 3,
            base_color: "#b5654f".into(),
            roof_color: "#3a3a3a".into(),
            window_style: WindowStyle::Grid,
            seed: 11,
            parts: Vec::new(),
        }
    }

    fn tower() -> Recipe {
        Recipe {
            name: "Meridian Tower".into(),
            archetype: Archetype::Tower,
            footprint_w: 2,
            footprint_h: 2,
            floors: 24,
            base_color: "#9fb8c8".into(),
            roof_color: "#22242c".into(),
            window_style: WindowStyle::Grid,
            seed: 33,
            parts: Vec::new(),
        }
    }

    fn placement(def_id: AssetId, origin: Cell, rotation: u8) -> Placement {
        Placement {
            def_id,
            revision: 1,
            origin,
            rotation,
        }
    }

    /// A world with two catalogue entries, two buildings (different shapes and
    /// rotations), an advanced tick, and an allocated asset id.
    fn populated() -> (WorldState, AssetCatalogue) {
        let mut catalogue = AssetCatalogue::new();
        catalogue.insert(AssetId(1), validate_recipe(&cedar()).unwrap());
        catalogue.insert(AssetId(2), validate_recipe(&tower()).unwrap());
        let mut world = WorldState::new();

        let _ = world.next_asset_id(); // id 1
        let _ = world.next_asset_id(); // id 2, counter now at 3
        world.advance_tick();
        world.advance_tick(); // tick 2

        let cedar_def = catalogue.get(AssetId(1)).unwrap();
        let first = world.next_building_id();
        apply_placement(
            &mut world,
            cedar_def,
            &placement(AssetId(1), Cell { x: 4, y: 5 }, 0),
            first,
        );
        let tower_def = catalogue.get(AssetId(2)).unwrap();
        let second = world.next_building_id();
        apply_placement(
            &mut world,
            tower_def,
            &placement(AssetId(2), Cell { x: 10, y: 2 }, 1),
            second,
        );
        (world, catalogue)
    }

    #[test]
    fn save_load_roundtrip_preserves_everything() {
        let (world, catalogue) = populated();
        let save = to_save(&world, &catalogue);

        let (restored_world, restored_catalogue) = from_save(&save).unwrap();

        assert_eq!(restored_world.tick_count(), world.tick_count());
        assert_eq!(
            restored_world.peek_next_building_id(),
            world.peek_next_building_id()
        );
        assert_eq!(
            restored_world.peek_next_asset_id(),
            world.peek_next_asset_id()
        );
        assert_eq!(
            restored_catalogue.iter().collect::<Vec<_>>(),
            catalogue.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            restored_world.buildings().collect::<Vec<_>>(),
            world.buildings().collect::<Vec<_>>()
        );

        // The restored cell index serves lookups like the original.
        assert_eq!(
            restored_world.occupant_at(Cell { x: 4, y: 5 }),
            Some(BuildingId(1))
        );
        assert_eq!(
            restored_world.occupant_at(Cell { x: 11, y: 3 }),
            Some(BuildingId(2)),
            "tower placed at rotation 1 covers 2 wide × 3 tall from (10,2)"
        );
        assert_eq!(restored_world.occupant_at(Cell { x: 9, y: 2 }), None);

        // The restored world keeps allocating fresh ids and ticks.
        let mut restored_world = restored_world;
        assert_eq!(restored_world.next_building_id(), BuildingId(3));
        assert_eq!(restored_world.next_asset_id(), AssetId(3));
        restored_world.advance_tick();
        assert_eq!(restored_world.tick_count(), 3);
    }

    #[test]
    fn empty_save_restores_a_fresh_world() {
        let save = WorldSave {
            tick: 0,
            next_building_id: 1,
            next_asset_id: 1,
            buildings: vec![],
            assets: vec![],
        };
        let (world, catalogue) = from_save(&save).unwrap();
        assert_eq!(world.tick_count(), 0);
        assert_eq!(world.buildings().next(), None);
        assert_eq!(catalogue.iter().next(), None);
    }

    #[test]
    fn rejects_building_referencing_unknown_asset() {
        let (world, catalogue) = populated();
        let mut save = to_save(&world, &catalogue);
        save.buildings[0].def_id = 99;
        let error = from_save(&save).unwrap_err();
        assert!(error.contains("unknown asset"), "got: {error}");
    }

    #[test]
    fn rejects_duplicate_cells() {
        let (world, catalogue) = populated();
        let mut save = to_save(&world, &catalogue);
        // Same origin as building 1 (3×2 box), so every cell collides.
        save.buildings.push(SaveBuilding {
            id: 10,
            def_id: 1,
            revision: 1,
            cell: Cell { x: 4, y: 5 },
            rotation: 0,
        });
        save.next_building_id = 11; // keep the counter coherent

        let error = from_save(&save).unwrap_err();
        assert!(error.contains("already held"), "got: {error}");
    }

    #[test]
    fn rejects_partially_overlapping_cells() {
        let (world, catalogue) = populated();
        let mut save = to_save(&world, &catalogue);
        // 3×2 at (3,5) shares only column x=4 with building 1 at (4,5).
        save.buildings.push(SaveBuilding {
            id: 11,
            def_id: 1,
            revision: 1,
            cell: Cell { x: 3, y: 5 },
            rotation: 0,
        });
        save.next_building_id = 12; // keep the counter coherent

        let error = from_save(&save).unwrap_err();
        assert!(error.contains("already held"), "got: {error}");
    }

    #[test]
    fn rejects_out_of_bounds_cells() {
        let (world, catalogue) = populated();
        let mut save = to_save(&world, &catalogue);
        save.buildings.push(SaveBuilding {
            id: 12,
            def_id: 1,
            revision: 1,
            cell: Cell { x: 95, y: 95 },
            rotation: 0,
        });
        save.next_building_id = 13; // keep the counter coherent

        let error = from_save(&save).unwrap_err();
        assert!(error.contains("outside the grid"), "got: {error}");
    }

    #[test]
    fn rejects_rotation_above_three() {
        let (world, catalogue) = populated();
        let mut save = to_save(&world, &catalogue);
        save.buildings[0].rotation = 4;
        let error = from_save(&save).unwrap_err();
        assert!(error.contains("rotation"), "got: {error}");
    }

    #[test]
    fn rejects_duplicate_building_ids() {
        let (world, catalogue) = populated();
        let mut save = to_save(&world, &catalogue);
        let mut second = save.buildings[0].clone();
        second.cell = Cell { x: 40, y: 40 }; // no cell overlap, same id
        save.buildings.push(second);
        let error = from_save(&save).unwrap_err();
        assert!(error.contains("duplicate building id"), "got: {error}");
    }

    #[test]
    fn rejects_duplicate_asset_ids() {
        let (world, catalogue) = populated();
        let mut save = to_save(&world, &catalogue);
        let mut second = save.assets[0].clone();
        second.recipe.name = "Renamed".into();
        save.assets.push(second);
        let error = from_save(&save).unwrap_err();
        assert!(error.contains("duplicate asset id"), "got: {error}");
    }

    #[test]
    fn rejects_id_counters_behind_restored_ids() {
        let (world, catalogue) = populated();
        let mut save = to_save(&world, &catalogue);
        save.next_building_id = 1;
        let error = from_save(&save).unwrap_err();
        assert!(error.contains("next_building_id"), "got: {error}");

        let mut save = to_save(&world, &catalogue);
        save.next_asset_id = save.assets[0].id;
        let error = from_save(&save).unwrap_err();
        assert!(error.contains("next_asset_id"), "got: {error}");
    }
}
