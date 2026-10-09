//! Domain ↔ wire conversion. The only place that knows both
//! `city_simulation` domain types and `city_protocol` wire DTOs.
//!
//! Wire IDs are decimal strings (`WireId`); domain IDs are `u64` newtypes.
//! Every wire → domain parse failure maps to a `bad_request` result so the
//! client always gets an explicit, typed rejection.

use city_protocol as wire;
use city_simulation::{
    ApprovedDefinition, AssetCatalogue, AssetId, Building, Cell, Reject, WorldState,
};

/// Definitions are immutable once published; M1 publishes each new recipe
/// under a fresh id at revision 1. The persistence schema stores no revision
/// column, so this constant is the single source of revision truth.
pub(crate) const PUBLISHED_REVISION: u32 = 1;

/// The only actor in M1: the single authenticated owner connection.
pub(crate) const OWNER_ACTOR: &str = "owner";

/// Rebuilds a full world snapshot for `hello` / `GET` consumers.
/// Lists are sorted by id so snapshots are deterministic.
pub(crate) fn world_dto(world: &WorldState, catalogue: &AssetCatalogue) -> wire::WorldDto {
    let mut buildings: Vec<_> = world.buildings().collect();
    buildings.sort_by_key(|b| b.id);
    let buildings = buildings
        .into_iter()
        .map(|building| {
            let def = catalogue
                .get(building.def_id)
                .unwrap_or_else(|| panic!("catalogue missing definition {}", building.def_id.0));
            building_dto(building, def)
        })
        .collect();
    let mut definitions: Vec<_> = catalogue.iter().collect();
    definitions.sort_by_key(|(id, _)| *id);
    let definitions = definitions
        .into_iter()
        .map(|(id, approved)| definition_dto(id, approved))
        .collect();
    wire::WorldDto {
        grid: wire::GridDto {
            width: city_simulation::GRID_WIDTH,
            height: city_simulation::GRID_HEIGHT,
        },
        tick: world.tick_count(),
        buildings,
        definitions,
    }
}

pub(crate) fn definition_dto(id: AssetId, approved: &ApprovedDefinition) -> wire::AssetDefinition {
    wire::AssetDefinition {
        id: wire_id(id.0),
        revision: PUBLISHED_REVISION,
        recipe: approved.recipe.clone(),
        derived: approved.derived,
    }
}

pub(crate) fn building_dto(building: &Building, def: &ApprovedDefinition) -> wire::BuildingDto {
    wire::BuildingDto {
        id: wire_id(building.id.0),
        def_id: wire_id(building.def_id.0),
        revision: building.revision,
        cell: wire::Cell {
            x: building.origin.x,
            y: building.origin.y,
        },
        rotation: building.rotation,
        derived: def.derived,
    }
}

/// Domain id → decimal wire id.
pub(crate) fn wire_id(id: u64) -> wire::WireId {
    id.to_string()
}

/// Decimal wire id → domain id. Only the canonical form is accepted:
/// non-empty, ASCII digits only (no sign, whitespace, or overflow).
pub(crate) fn parse_wire_id(raw: &str) -> Result<u64, String> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("id must be a decimal string, got {raw:?}"));
    }
    raw.parse::<u64>()
        .map_err(|_| format!("id {raw:?} exceeds the u64 range"))
}

/// Parses one inbound WS text frame. Malformed input (bad JSON, unknown
/// shape) becomes a `bad_request` result addressed back to the same socket;
/// `command_id` is sniffed out of the raw JSON when possible so the client
/// can correlate the rejection with its pending command.
pub(crate) fn parse_client_message(text: &str) -> Result<wire::ClientMessage, wire::ServerMessage> {
    match serde_json::from_str(text) {
        Ok(message) => Ok(message),
        Err(err) => Err(bad_request_message(&sniff_command_id(text), &err)),
    }
}

fn sniff_command_id(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| {
            value
                .get("command")
                .and_then(|command| command.get("command_id"))
                .or_else(|| value.get("command_id"))
                .and_then(|id| id.as_str().map(str::to_owned))
        })
        .unwrap_or_default()
}

/// Canonical receipt payload: re-serializing the typed command gives a
/// byte-stable form, so semantically identical commands (differing only in
/// JSON whitespace or key order) compare equal.
pub(crate) fn canonical_command(command: &wire::Command) -> String {
    serde_json::to_string(command).expect("Command serialization cannot fail")
}

/// The `command_id` carried by either command variant.
pub(crate) fn command_id(command: &wire::Command) -> &str {
    match command {
        wire::Command::PlaceBuilding { command_id, .. } => command_id,
        wire::Command::DemolishBuilding { command_id, .. } => command_id,
    }
}

/// Validates the wire-only parts of a placement that the domain cannot see
/// (decimal id syntax, published revision, rotation range). Everything
/// geometric goes through `simulation::construction::validate_placement`.
pub(crate) fn parse_placement(
    def_id: &str,
    revision: u32,
    cell: wire::Cell,
    rotation: u8,
) -> Result<city_simulation::Placement, String> {
    let def_id = parse_wire_id(def_id).map_err(|err| format!("def_id invalid: {err}"))?;
    if revision != PUBLISHED_REVISION {
        return Err(format!(
            "definition revision {revision} not found (current revision is {PUBLISHED_REVISION})"
        ));
    }
    if rotation > 3 {
        return Err(format!("rotation must be 0..=3, got {rotation}"));
    }
    Ok(city_simulation::Placement {
        def_id: AssetId(def_id),
        revision,
        origin: Cell {
            x: cell.x,
            y: cell.y,
        },
        rotation,
    })
}

/// Maps a domain rejection onto the wire, including the conflicting
/// building id for overlaps. Messages mirror the contract's phrasing so
/// clients can show them directly.
pub(crate) fn reject_result(command_id: &str, reject: Reject) -> wire::CommandResult {
    let (code, message, conflicting) = match reject {
        Reject::Overlap { conflicting } => (
            wire::RejectCode::Overlap,
            format!("footprint overlaps building {}", conflicting.0),
            Some(wire_id(conflicting.0)),
        ),
        Reject::OutOfBounds(width, height) => (
            wire::RejectCode::OutOfBounds,
            format!("footprint extends outside the {width}x{height} grid"),
            None,
        ),
        Reject::UnknownAsset(id) => (
            wire::RejectCode::UnknownAsset,
            format!("unknown asset definition {}", id.0),
            None,
        ),
        Reject::UnknownBuilding(id) => (
            wire::RejectCode::UnknownBuilding,
            format!("unknown building {}", id.0),
            None,
        ),
    };
    wire::CommandResult::Rejected {
        command_id: command_id.to_owned(),
        code,
        message,
        conflicting_building_id: conflicting,
    }
}

/// A `bad_request` rejection shaped for the wire.
pub(crate) fn bad_request(command_id: &str, message: impl Into<String>) -> wire::CommandResult {
    wire::CommandResult::Rejected {
        command_id: command_id.to_owned(),
        code: wire::RejectCode::BadRequest,
        message: message.into(),
        conflicting_building_id: None,
    }
}

fn bad_request_message(command_id: &str, err: &serde_json::Error) -> wire::ServerMessage {
    wire::ServerMessage::CommandResult {
        result: bad_request(
            command_id,
            format!(
                "malformed message, expected {{\"type\":\"command\",\"command\":{{...}}}}: {err}"
            ),
        ),
    }
}

/// Wraps a committed outcome as the server→client reply for `conn`.
pub(crate) fn result_message(result: wire::CommandResult) -> wire::ServerMessage {
    wire::ServerMessage::CommandResult { result }
}

/// Wraps a domain event for broadcast.
pub(crate) fn event_message(event: wire::Event) -> wire::ServerMessage {
    wire::ServerMessage::Event { event }
}

#[cfg(test)]
mod tests {
    use super::*;
    use city_protocol::{Command, Recipe};
    use city_simulation::BuildingId;

    #[test]
    fn parses_valid_command_message() {
        let text = r#"{"type":"command","command":{"type":"place_building","command_id":"c1","def_id":"1","revision":1,"cell":{"x":10,"y":12},"rotation":0}}"#;
        let message = parse_client_message(text).ok().expect("valid command");
        let wire::ClientMessage::Command { command } = message;
        assert_eq!(command_id(&command), "c1");
    }

    #[test]
    fn malformed_json_yields_bad_request_with_sniffed_id() {
        let message = parse_client_message("{not json").err().expect("rejected");
        let wire::ServerMessage::CommandResult { result } = message else {
            panic!("expected command result");
        };
        let wire::CommandResult::Rejected {
            command_id,
            code,
            conflicting_building_id,
            ..
        } = result
        else {
            panic!("expected rejection");
        };
        assert_eq!(command_id, "");
        assert_eq!(code, wire::RejectCode::BadRequest);
        assert_eq!(conflicting_building_id, None);
    }

    #[test]
    fn unknown_shape_yields_bad_request_and_keeps_command_id() {
        let text = r#"{"type":"command","command":{"type":"teleport","command_id":"c7"}}"#;
        let message = parse_client_message(text).err().expect("rejected");
        let wire::ServerMessage::CommandResult { result } = message else {
            panic!("expected command result");
        };
        let wire::CommandResult::Rejected {
            command_id, code, ..
        } = result
        else {
            panic!("expected rejection");
        };
        assert_eq!(command_id, "c7");
        assert_eq!(code, wire::RejectCode::BadRequest);
    }

    #[test]
    fn canonical_payload_ignores_key_order_and_whitespace() {
        let a = parse_client_message(
            r#"{"type":"command","command":{"type":"place_building","command_id":"c1","def_id":"1","revision":1,"cell":{"x":1,"y":2},"rotation":0}}"#,
        )
        .ok()
        .unwrap();
        let b = parse_client_message(
            r#"{"command": {"type": "place_building", "rotation": 0, "cell": {"y": 2, "x": 1}, "revision": 1, "def_id": "1", "command_id": "c1"}, "type": "command"}"#,
        )
        .ok()
        .unwrap();
        let wire::ClientMessage::Command { command: a } = a;
        let wire::ClientMessage::Command { command: b } = b;
        assert_eq!(canonical_command(&a), canonical_command(&b));
        // A different payload under the same id must differ.
        let mut c = a.clone();
        let Command::PlaceBuilding { cell, .. } = &mut c else {
            panic!()
        };
        cell.x = 5;
        assert_ne!(canonical_command(&a), canonical_command(&c));
    }

    #[test]
    fn parse_placement_rejects_bad_wire_values() {
        assert!(parse_placement("abc", 1, wire::Cell { x: 0, y: 0 }, 0).is_err());
        assert!(parse_placement("1", 2, wire::Cell { x: 0, y: 0 }, 0).is_err());
        assert!(parse_placement("1", 1, wire::Cell { x: 0, y: 0 }, 4).is_err());
        let placement = parse_placement("3", 1, wire::Cell { x: -1, y: 20 }, 3)
            .ok()
            .unwrap();
        assert_eq!(placement.def_id, AssetId(3));
        assert_eq!(placement.revision, 1);
        assert_eq!(placement.origin, Cell { x: -1, y: 20 });
        assert_eq!(placement.rotation, 3);
    }

    #[test]
    fn parse_wire_id_is_strict() {
        assert_eq!(parse_wire_id("42").ok(), Some(42));
        assert!(parse_wire_id("").is_err());
        assert!(parse_wire_id("+5").is_err());
        assert!(parse_wire_id("5 ").is_err());
        assert!(parse_wire_id("0x10").is_err());
        assert!(parse_wire_id("99999999999999999999999").is_err());
    }

    #[test]
    fn reject_mapping_carries_conflicting_building() {
        let result = reject_result(
            "c1",
            Reject::Overlap {
                conflicting: BuildingId(3),
            },
        );
        let wire::CommandResult::Rejected {
            code,
            message,
            conflicting_building_id,
            ..
        } = result
        else {
            panic!("expected rejection");
        };
        assert_eq!(code, wire::RejectCode::Overlap);
        assert_eq!(message, "footprint overlaps building 3");
        assert_eq!(conflicting_building_id.as_deref(), Some("3"));
    }

    #[test]
    fn seed_recipe_shape_roundtrips_through_json() {
        // Guards against silent serde drift in the wire Recipe shape used by
        // POST /api/assets.
        let text = r##"{"name":"Cedar Walk-up","archetype":"house","footprint_w":3,"footprint_h":2,"floors":3,"base_color":"#b5654f","roof_color":"#3a3a3a","window_style":"grid","seed":11}"##;
        let recipe: Recipe = serde_json::from_str(text).unwrap();
        assert_eq!(recipe.name, "Cedar Walk-up");
        assert_eq!(recipe.archetype, city_protocol::Archetype::House);
    }
}
