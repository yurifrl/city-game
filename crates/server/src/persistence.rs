//! SQLite persistence. One `Store` owns the single connection; it lives on
//! the world runner so all reads and writes happen in command order.
//!
//! Schema is normative: docs/contracts/milestone-1.md §Persistence schema.
//! WAL + `synchronous=FULL`; every accepted command commits its mutation,
//! the meta counters, and its receipt in exactly one transaction before any
//! broadcast happens.

use std::path::Path;

use city_protocol::{DerivedStats, Recipe};
use city_simulation::{AssetCatalogue, AssetId, BuildingId, SaveAsset, SaveBuilding, WorldSave, WorldState, from_save};
use rusqlite::{Connection, OptionalExtension, Transaction};

pub(crate) const SCHEMA_VERSION: &str = "1";

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS assets (
  id INTEGER PRIMARY KEY,
  recipe_json TEXT NOT NULL,
  capacity INTEGER NOT NULL,
  cost_credits INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS buildings (
  id INTEGER PRIMARY KEY,
  def_id INTEGER NOT NULL,
  revision INTEGER NOT NULL,
  cell_x INTEGER NOT NULL,
  cell_y INTEGER NOT NULL,
  rotation INTEGER NOT NULL,
  command_id TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS receipts (
  command_id TEXT PRIMARY KEY,
  payload TEXT NOT NULL,
  outcome_json TEXT NOT NULL
);
";

/// The seed catalogue inserted exactly once, on an empty database.
/// Values are fixed by docs/contracts/milestone-1.md (ids 1, 2, 3).
pub(crate) fn seed_recipes() -> [Recipe; 3] {
    [
        Recipe {
            name: "Cedar Walk-up".to_owned(),
            archetype: city_protocol::Archetype::House,
            footprint_w: 3,
            footprint_h: 2,
            floors: 3,
            base_color: "#b5654f".to_owned(),
            roof_color: "#3a3a3a".to_owned(),
            window_style: city_protocol::WindowStyle::Grid,
            seed: 11,
        },
        Recipe {
            name: "Harbor Point Office".to_owned(),
            archetype: city_protocol::Archetype::Office,
            footprint_w: 3,
            footprint_h: 3,
            floors: 8,
            base_color: "#7f9bb3".to_owned(),
            roof_color: "#2f2f38".to_owned(),
            window_style: city_protocol::WindowStyle::Stripes,
            seed: 22,
        },
        Recipe {
            name: "Meridian Tower".to_owned(),
            archetype: city_protocol::Archetype::Tower,
            footprint_w: 2,
            footprint_h: 2,
            floors: 24,
            base_color: "#9fb8c8".to_owned(),
            roof_color: "#22242c".to_owned(),
            window_style: city_protocol::WindowStyle::Grid,
            seed: 33,
        },
    ]
}

pub(crate) struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) the database, enables WAL + FULL
    /// synchronous, and ensures the schema exists.
    pub(crate) fn open(path: &Path) -> Result<Store, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("cannot create data dir {}: {err}", parent.display()))?;
        }
        let conn =
            Connection::open(path).map_err(|err| format!("cannot open {}: {err}", path.display()))?;
        let mode: String = conn
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
            .map_err(|err| format!("cannot enable WAL: {err}"))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(format!("journal_mode is {mode:?}, expected WAL"));
        }
        conn.execute_batch("PRAGMA synchronous=FULL;")
            .map_err(|err| format!("cannot enable synchronous=FULL: {err}"))?;
        conn.execute_batch(SCHEMA_SQL)
            .map_err(|err| format!("cannot create schema: {err}"))?;
        Ok(Store { conn })
    }

    /// True when the database has never been initialized (no schema_version).
    pub(crate) fn is_empty(&self) -> Result<bool, String> {
        Ok(self.meta("schema_version")?.is_none())
    }

    fn meta(&self, key: &str) -> Result<Option<String>, String> {
        self.conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| row.get(0))
            .optional()
            .map_err(|err| format!("cannot read meta {key}: {err}"))
    }

    fn set_meta(tx: &Transaction, key: &str, value: &str) -> Result<(), String> {
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key, value],
        )
        .map_err(|err| format!("cannot write meta {key}: {err}"))?;
        Ok(())
    }

    /// Seeds the three stock definitions (ids 1..=3) plus the meta counters.
    /// Only valid on an empty database; derived stats come from the domain's
    /// `validate_recipe` so the derivation table stays the single authority.
    pub(crate) fn seed(&self) -> Result<(), String> {
        if !self.is_empty()? {
            return Err("refusing to seed a non-empty database".to_owned());
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|err| format!("cannot begin seed transaction: {err}"))?;
        for (index, recipe) in seed_recipes().iter().enumerate() {
            let approved = city_simulation::assets::validate_recipe(recipe)
                .map_err(|errors| format!("seed recipe {:?} invalid: {errors:?}", recipe.name))?;
            let id = (index as u64) + 1;
            tx.execute(
                "INSERT INTO assets (id, recipe_json, capacity, cost_credits) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    id,
                    serde_json::to_string(recipe).expect("Recipe serialization cannot fail"),
                    approved.derived.capacity,
                    approved.derived.cost_credits
                ],
            )
            .map_err(|err| format!("cannot insert seed asset {id}: {err}"))?;
        }
        Self::set_meta(&tx, "schema_version", SCHEMA_VERSION)?;
        Self::set_meta(&tx, "tick", "0")?;
        Self::set_meta(&tx, "next_building_id", "1")?;
        Self::set_meta(&tx, "next_asset_id", "4")?;
        tx.commit().map_err(|err| format!("cannot commit seed: {err}"))?;
        Ok(())
    }

    /// Reads the durable tables back into a `WorldSave` for boot/resync.
    pub(crate) fn load(&self) -> Result<WorldSave, String> {
        let tick = self.meta("tick")?.ok_or("meta missing tick")?;
        let tick = tick.parse().map_err(|_| format!("meta tick {tick:?} is not u64"))?;
        let next_building_id = self
            .meta("next_building_id")?
            .ok_or("meta missing next_building_id")?;
        let next_building_id = next_building_id
            .parse()
            .map_err(|_| format!("meta next_building_id {next_building_id:?} is not u64"))?;
        let next_asset_id = self.meta("next_asset_id")?.ok_or("meta missing next_asset_id")?;
        let next_asset_id = next_asset_id
            .parse()
            .map_err(|_| format!("meta next_asset_id {next_asset_id:?} is not u64"))?;

        let mut statement = self
            .conn
            .prepare("SELECT id, recipe_json, capacity, cost_credits FROM assets ORDER BY id")
            .map_err(|err| format!("cannot read assets: {err}"))?;
        let asset_rows: Vec<(i64, String, i64, i64)> = statement
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                ))
            })
            .map_err(|err| format!("cannot read assets: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("cannot read asset row: {err}"))?;
        let assets = asset_rows
            .into_iter()
            .map(|(id, recipe_json, capacity, cost_credits)| {
                Ok(SaveAsset {
                    id: id as u64,
                    recipe: serde_json::from_str(&recipe_json)
                        .map_err(|err| format!("asset {id} recipe_json corrupted: {err}"))?,
                    derived: DerivedStats {
                        capacity: capacity as u32,
                        cost_credits: cost_credits as u64,
                    },
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        let mut statement = self
            .conn
            .prepare("SELECT id, def_id, revision, cell_x, cell_y, rotation FROM buildings ORDER BY id")
            .map_err(|err| format!("cannot read buildings: {err}"))?;
        let buildings = statement
            .query_map([], |row| {
                Ok(SaveBuilding {
                    id: row.get::<_, i64>(0)? as u64,
                    def_id: row.get::<_, i64>(1)? as u64,
                    revision: row.get::<_, i64>(2)? as u32,
                    cell: city_simulation::Cell {
                        x: row.get::<_, i64>(3)? as i32,
                        y: row.get::<_, i64>(4)? as i32,
                    },
                    rotation: row.get::<_, i64>(5)? as u8,
                })
            })
            .map_err(|err| format!("cannot read buildings: {err}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| format!("cannot read building row: {err}"))?;

        Ok(WorldSave {
            tick,
            next_building_id,
            next_asset_id,
            buildings,
            assets,
        })
    }

    /// Boot path: tables → `WorldSave` → domain state. Any inconsistency in
    /// the records is an error, never a silent default.
    pub(crate) fn reload(&self) -> Result<(WorldState, AssetCatalogue), String> {
        let save = self.load()?;
        from_save(&save).map_err(|err| format!("save is inconsistent: {err}"))
    }

    /// Stored receipt for `command_id`: `(payload, outcome)`.
    pub(crate) fn receipt(
        &self,
        command_id: &str,
    ) -> Result<Option<(String, city_protocol::CommandResult)>, String> {
        let row = self
            .conn
            .query_row(
                "SELECT payload, outcome_json FROM receipts WHERE command_id = ?1",
                [command_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(|err| format!("cannot read receipt {command_id}: {err}"))?;
        match row {
            None => Ok(None),
            Some((payload, outcome_json)) => {
                let outcome = serde_json::from_str(&outcome_json)
                    .map_err(|err| format!("receipt {command_id} has corrupt outcome_json: {err}"))?;
                Ok(Some((payload, outcome)))
            }
        }
    }

    /// One transaction: building row + meta counter + receipt. Called only
    /// after the domain accepted the placement, so any failure here is an
    /// infrastructure error the caller must resync from.
    pub(crate) fn commit_place(
        &self,
        building: &city_simulation::Building,
        next_building_id: u64,
        command_id: &str,
        payload: &str,
        outcome: &city_protocol::CommandResult,
    ) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|err| format!("cannot begin transaction: {err}"))?;
        tx.execute(
            "INSERT INTO buildings (id, def_id, revision, cell_x, cell_y, rotation, command_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                building.id.0 as i64,
                building.def_id.0 as i64,
                building.revision,
                building.origin.x,
                building.origin.y,
                building.rotation,
                command_id
            ],
        )
        .map_err(|err| format!("cannot insert building {}: {err}", building.id.0))?;
        Self::set_meta(&tx, "next_building_id", &next_building_id.to_string())?;
        Self::insert_receipt(&tx, command_id, payload, outcome)?;
        tx.commit().map_err(|err| format!("cannot commit placement: {err}"))?;
        Ok(())
    }

    /// One transaction: building row removal + receipt.
    pub(crate) fn commit_demolish(
        &self,
        building_id: BuildingId,
        command_id: &str,
        payload: &str,
        outcome: &city_protocol::CommandResult,
    ) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|err| format!("cannot begin transaction: {err}"))?;
        tx.execute("DELETE FROM buildings WHERE id = ?1", [building_id.0 as i64])
            .map_err(|err| format!("cannot delete building {}: {err}", building_id.0))?;
        Self::insert_receipt(&tx, command_id, payload, outcome)?;
        tx.commit().map_err(|err| format!("cannot commit demolition: {err}"))?;
        Ok(())
    }

    /// One transaction: asset row + meta counter.
    pub(crate) fn commit_asset(&self, id: AssetId, recipe: &Recipe, derived: &DerivedStats) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|err| format!("cannot begin transaction: {err}"))?;
        tx.execute(
            "INSERT INTO assets (id, recipe_json, capacity, cost_credits) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                id.0 as i64,
                serde_json::to_string(recipe).expect("Recipe serialization cannot fail"),
                derived.capacity,
                derived.cost_credits
            ],
        )
        .map_err(|err| format!("cannot insert asset {}: {err}", id.0))?;
        Self::set_meta(&tx, "next_asset_id", &(id.0 + 1).to_string())?;
        tx.commit().map_err(|err| format!("cannot commit asset: {err}"))?;
        Ok(())
    }

    /// Persists the outcome of a rejected-but-well-formed command so a
    /// resubmission replays the same verdict instead of re-deciding.
    pub(crate) fn commit_rejection(
        &self,
        command_id: &str,
        payload: &str,
        outcome: &city_protocol::CommandResult,
    ) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|err| format!("cannot begin transaction: {err}"))?;
        Self::insert_receipt(&tx, command_id, payload, outcome)?;
        tx.commit().map_err(|err| format!("cannot commit rejection: {err}"))?;
        Ok(())
    }

    fn insert_receipt(
        tx: &Transaction,
        command_id: &str,
        payload: &str,
        outcome: &city_protocol::CommandResult,
    ) -> Result<(), String> {
        tx.execute(
            "INSERT INTO receipts (command_id, payload, outcome_json) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                command_id,
                payload,
                serde_json::to_string(outcome).expect("CommandResult serialization cannot fail")
            ],
        )
        .map_err(|err| format!("cannot insert receipt {command_id}: {err}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn store_in(dir: &Path) -> Store {
        Store::open(&dir.join("city.db")).expect("store opens")
    }

    #[test]
    fn open_creates_schema_with_wal_and_full_sync() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let journal: String = store
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal.to_ascii_lowercase(), "wal");
        let sync: i64 = store
            .conn
            .query_row("PRAGMA synchronous", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sync, 2, "2 = synchronous=FULL");
        let mut statement = store
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(tables, ["assets", "buildings", "meta", "receipts"]);
    }

    #[test]
    fn empty_database_is_detected_and_seeded_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        assert!(store.is_empty().unwrap());
        store.seed().unwrap();
        assert!(!store.is_empty().unwrap());
        assert!(store.seed().is_err(), "seeding twice must fail loudly");

        // Meta counters per contract.
        assert_eq!(store.meta("schema_version").unwrap().as_deref(), Some("1"));
        assert_eq!(store.meta("tick").unwrap().as_deref(), Some("0"));
        assert_eq!(store.meta("next_building_id").unwrap().as_deref(), Some("1"));
        assert_eq!(store.meta("next_asset_id").unwrap().as_deref(), Some("4"));

        let save = store.load().unwrap();
        assert_eq!(save.assets.len(), 3);
        assert_eq!(save.assets[0].id, 1);
        assert_eq!(save.assets[0].recipe.name, "Cedar Walk-up");
        assert_eq!(save.assets[2].recipe.name, "Meridian Tower");
        assert!(save.buildings.is_empty());
    }

    #[test]
    fn receipts_roundtrip_outcomes() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let payload = r#"{"type":"place_building","command_id":"c1","def_id":"1","revision":1,"cell":{"x":0,"y":0},"rotation":0}"#;
        let outcome = city_protocol::CommandResult::Accepted {
            command_id: "c1".to_owned(),
            building_id: "7".to_owned(),
        };
        store.commit_rejection("c1", payload, &outcome).unwrap();

        let (stored_payload, stored_outcome) =
            store.receipt("c1").unwrap().expect("receipt exists");
        assert_eq!(stored_payload, payload);
        assert_eq!(stored_outcome, outcome);
        assert!(store.receipt("missing").unwrap().is_none());
    }

    #[test]
    fn place_then_demolish_commits_persist() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        store.seed().unwrap();

        let payload = r#"{"type":"place_building","command_id":"p1","def_id":"1","revision":1,"cell":{"x":4,"y":4},"rotation":0}"#;
        let building = city_simulation::Building {
            id: BuildingId(1),
            def_id: AssetId(1),
            revision: 1,
            origin: city_simulation::Cell { x: 4, y: 4 },
            rotation: 0,
        };
        let outcome = city_protocol::CommandResult::Accepted {
            command_id: "p1".to_owned(),
            building_id: "1".to_owned(),
        };
        store.commit_place(&building, 2, "p1", payload, &outcome).unwrap();

        let save = store.load().unwrap();
        assert_eq!(save.buildings.len(), 1);
        assert_eq!(save.buildings[0].id, 1);
        assert_eq!(save.buildings[0].def_id, 1);
        assert_eq!(save.buildings[0].cell, city_simulation::Cell { x: 4, y: 4 });
        assert_eq!(save.next_building_id, 2);

        store
            .commit_demolish(
                BuildingId(1),
                "d1",
                r#"{"type":"demolish_building","command_id":"d1","building_id":"1"}"#,
                &outcome,
            )
            .unwrap();
        let save = store.load().unwrap();
        assert!(save.buildings.is_empty());
        assert_eq!(save.next_asset_id, 4, "asset counter untouched by building commits");
        assert!(store.receipt("p1").unwrap().is_some());
        assert!(store.receipt("d1").unwrap().is_some());
    }

    #[test]
    fn corrupt_receipt_outcome_fails_loudly() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        store
            .conn
            .execute(
                "INSERT INTO receipts (command_id, payload, outcome_json) VALUES ('c9', '{}', 'not-json')",
                [],
            )
            .unwrap();
        let err = store.receipt("c9").unwrap_err();
        assert!(err.contains("corrupt"), "got: {err}");
    }
}
