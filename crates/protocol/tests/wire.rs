//! Wire-shape tests: the JSON frames defined in docs/contracts/multiplayer-stream.md
//! are normative, so every frame here must roundtrip byte for byte.

use city_protocol::{
    Archetype, AssetDefinition, BuildingDto, Cell, ClientMessage, Command, CommandResult,
    DerivedStats, Event, GridDto, InputKind, InputMessage, PresenceKind, PresenceState, Recipe,
    RejectCode, ServerMessage, WindowStyle, WorldDto,
};

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

fn cell(x: i32, y: i32) -> Cell {
    Cell { x, y }
}

fn building(id: &str, owner: Option<&str>) -> BuildingDto {
    BuildingDto {
        id: id.into(),
        def_id: "1".into(),
        revision: 1,
        cell: Cell { x: 10, y: 12 },
        rotation: 0,
        derived: DerivedStats {
            capacity: 2,
            cost_credits: 2160,
        },
        owner: owner.map(Into::into),
    }
}

/// Client → server frames must match the contract byte for byte, including
/// the `type` tags and snake_case field names.
#[test]
fn command_frames_match_contract_json() {
    let place = ClientMessage::Command {
        command: Command::PlaceBuilding {
            command_id: "c1".into(),
            def_id: "1".into(),
            revision: 1,
            cell: Cell { x: 10, y: 12 },
            rotation: 0,
        },
    };
    assert_eq!(
        serde_json::to_string(&place).unwrap(),
        r#"{"type":"command","command":{"type":"place_building","command_id":"c1","def_id":"1","revision":1,"cell":{"x":10,"y":12},"rotation":0}}"#
    );

    let demolish = ClientMessage::Command {
        command: Command::DemolishBuilding {
            command_id: "c2".into(),
            building_id: "7".into(),
        },
    };
    assert_eq!(
        serde_json::to_string(&demolish).unwrap(),
        r#"{"type":"command","command":{"type":"demolish_building","command_id":"c2","building_id":"7"}}"#
    );

    let clear = ClientMessage::Command {
        command: Command::ClearBuildings {
            command_id: "c3".into(),
        },
    };
    assert_eq!(
        serde_json::to_string(&clear).unwrap(),
        r#"{"type":"command","command":{"type":"clear_buildings","command_id":"c3"}}"#
    );
}

#[test]
fn command_frames_deserialize_from_contract_json() {
    let place: ClientMessage = serde_json::from_str(
        r#"{"type":"command","command":{"type":"place_building","command_id":"c1","def_id":"1","revision":1,"cell":{"x":10,"y":12},"rotation":0}}"#,
    )
    .unwrap();
    assert_eq!(
        place,
        ClientMessage::Command {
            command: Command::PlaceBuilding {
                command_id: "c1".into(),
                def_id: "1".into(),
                revision: 1,
                cell: Cell { x: 10, y: 12 },
                rotation: 0,
            },
        }
    );

    let demolish: ClientMessage = serde_json::from_str(
        r#"{"type":"command","command":{"type":"demolish_building","command_id":"c2","building_id":"7"}}"#,
    )
    .unwrap();
    assert_eq!(
        demolish,
        ClientMessage::Command {
            command: Command::DemolishBuilding {
                command_id: "c2".into(),
                building_id: "7".into(),
            },
        }
    );
}

#[test]
fn wire_ids_stay_decimal_strings() {
    let dto = building("9007199254740993", None);
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains(r#""id":"9007199254740993""#), "got: {json}");
    assert!(json.contains(r#""def_id":"1""#), "got: {json}");
    assert_eq!(serde_json::from_str::<BuildingDto>(&json).unwrap(), dto);
}

/// A v1 building (no owner key on the wire) still deserializes, and `owner`
/// is absent — never null — when there is none.
#[test]
fn building_owner_absent_when_none_and_survives_v1_payload() {
    let no_owner = building("1", None);
    let json = serde_json::to_string(&no_owner).unwrap();
    assert!(!json.contains("owner"), "got: {json}");
    // v1 wire payload with no owner key must still parse (serde default).
    let parsed: BuildingDto = serde_json::from_str(
        r#"{"id":"1","def_id":"1","revision":1,"cell":{"x":10,"y":12},"rotation":0,"derived":{"capacity":2,"cost_credits":2160}}"#,
    )
    .unwrap();
    assert_eq!(parsed, no_owner);
    assert_eq!(parsed.owner, None);

    let owned = building("2", Some("a1b2"));
    let json = serde_json::to_string(&owned).unwrap();
    assert!(json.contains(r#""owner":"a1b2""#), "got: {json}");
    assert_eq!(serde_json::from_str::<BuildingDto>(&json).unwrap(), owned);
}

/// The first frame on a fresh connection confirms identity and carries no
/// world data; `helloworld` follows as its own frame.
#[test]
fn hello_frames_split_identity_from_world() {
    let hello = ServerMessage::Hello { you: "a1b2c3d4".into() };
    assert_eq!(
        serde_json::to_string(&hello).unwrap(),
        r#"{"type":"hello","you":"a1b2c3d4"}"#
    );
    let parsed: ServerMessage = serde_json::from_str(r#"{"type":"hello","you":"a1b2c3d4"}"#).unwrap();
    assert_eq!(parsed, hello);

    let world = WorldDto {
        grid: GridDto { width: 96, height: 96 },
        tick: 0,
        buildings: vec![],
        definitions: vec![],
    };
    let hello_world = ServerMessage::HelloWorld { world };
    assert_eq!(
        serde_json::to_string(&hello_world).unwrap(),
        r#"{"type":"helloworld","world":{"grid":{"width":96,"height":96},"tick":0,"buildings":[],"definitions":[]}}"#
    );
}

/// The old M1 combined hello frame must be gone: no `actor`/`world` keys in
/// `hello`, and no combined shape parses.
#[test]
fn hello_no_longer_carries_actor_or_world() {
    assert!(serde_json::from_str::<ServerMessage>(r#"{"type":"hello","actor":"owner","world":{"grid":{"width":96,"height":96},"tick":0,"buildings":[],"definitions":[]}}"#).is_err());
}

#[test]
fn identify_and_input_frames_roundtrip() {
    let identify = ClientMessage::Identify { key: "a1b2c3d4-0000-4000-8000-000000000000".into() };
    assert_eq!(
        serde_json::to_string(&identify).unwrap(),
        r#"{"type":"identify","key":"a1b2c3d4-0000-4000-8000-000000000000"}"#
    );
    assert_eq!(
        serde_json::from_str::<ClientMessage>(r#"{"type":"identify","key":"k"}"#).unwrap(),
        ClientMessage::Identify { key: "k".into() }
    );

    // Bare hover: no def fields at all.
    let move_frame = ClientMessage::Input {
        input: InputMessage {
            kind: InputKind::Move,
            cell: cell(3, 4),
            def_id: None,
            rotation: None,
        },
    };
    assert_eq!(
        serde_json::to_string(&move_frame).unwrap(),
        r#"{"type":"input","input":{"kind":"move","cell":{"x":3,"y":4}}}"#
    );
    let parsed: ClientMessage =
        serde_json::from_str(r#"{"type":"input","input":{"kind":"move","cell":{"x":3,"y":4}}}"#).unwrap();
    assert_eq!(parsed, move_frame);

    // Armed drag sample carries def_id + rotation.
    let drag = ClientMessage::Input {
        input: InputMessage {
            kind: InputKind::Move,
            cell: cell(5, 6),
            def_id: Some("12".into()),
            rotation: Some(2),
        },
    };
    assert_eq!(
        serde_json::to_string(&drag).unwrap(),
        r#"{"type":"input","input":{"kind":"move","cell":{"x":5,"y":6},"def_id":"12","rotation":2}}"#
    );
    let parsed: ClientMessage =
        serde_json::from_str(r#"{"type":"input","input":{"kind":"move","cell":{"x":5,"y":6},"def_id":"12","rotation":2}}"#).unwrap();
    assert_eq!(parsed, drag);
}

#[test]
fn event_frames_match_contract_shapes() {
    assert_eq!(
        serde_json::to_string(&ServerMessage::Event {
            event: Event::BuildingPlaced { building: building("1", Some("a1b2")) }
        })
        .unwrap(),
        concat!(
            r#"{"type":"event","event":{"type":"building_placed","#,
            r#""building":{"id":"1","def_id":"1","revision":1,"cell":{"x":10,"y":12},"rotation":0,"derived":{"capacity":2,"cost_credits":2160},"owner":"a1b2"}}}"#
        )
    );
    assert_eq!(
        serde_json::to_string(&ServerMessage::Event {
            event: Event::BuildingDemolished {
                building_id: "7".into(),
                actor: "a1b2c3d4".into(),
            }
        })
        .unwrap(),
        r#"{"type":"event","event":{"type":"building_demolished","building_id":"7","actor":"a1b2c3d4"}}"#
    );
    assert_eq!(
        serde_json::to_string(&ServerMessage::Event {
            event: Event::BuildingsCleared {
                removed: 3,
                actor: "a1b2c3d4".into(),
            }
        })
        .unwrap(),
        r#"{"type":"event","event":{"type":"buildings_cleared","removed":3,"actor":"a1b2c3d4"}}"#
    );

    let published = ServerMessage::Event {
        event: Event::DefinitionPublished {
            definition: AssetDefinition {
                id: "1".into(),
                revision: 1,
                recipe: cedar(),
                derived: DerivedStats {
                    capacity: 2,
                    cost_credits: 2160,
                },
            },
        },
    };
    assert_eq!(
        serde_json::to_value(&published).unwrap(),
        serde_json::json!({
            "type": "event",
            "event": {
                "type": "definition_published",
                "definition": {
                    "id": "1",
                    "revision": 1,
                    "recipe": {
                        "name": "Cedar Walk-up",
                        "archetype": "house",
                        "footprint_w": 3,
                        "footprint_h": 2,
                        "floors": 3,
                        "base_color": "#b5654f",
                        "roof_color": "#3a3a3a",
                        "window_style": "grid",
                        "seed": 11,
                        "parts": [],
                    },
                    "derived": { "capacity": 2, "cost_credits": 2160 },
                },
            },
        })
    );
}

#[test]
fn presence_frames_match_contract_shapes() {
    // Idle cursor: kind "idle", no def fields on the wire.
    let idle = ServerMessage::Event {
        event: Event::Presence {
            presence: PresenceState {
                actor: "a1b2c3d4".into(),
                label: "player-a1b2".into(),
                cell: cell(3, 4),
                kind: PresenceKind::Idle,
                def_id: None,
                rotation: None,
            },
        },
    };
    let idle_json = serde_json::to_string(&idle).unwrap();
    assert_eq!(
        idle_json,
        r#"{"type":"event","event":{"type":"presence","presence":{"actor":"a1b2c3d4","label":"player-a1b2","cell":{"x":3,"y":4},"kind":"idle"}}}"#
    );
    assert_eq!(serde_json::from_str::<ServerMessage>(&idle_json).unwrap(), idle);

    // Armed drag: kind "drag" plus def_id and rotation.
    let drag = ServerMessage::Event {
        event: Event::Presence {
            presence: PresenceState {
                actor: "a1b2c3d4".into(),
                label: "player-a1b2".into(),
                cell: cell(5, 6),
                kind: PresenceKind::Drag,
                def_id: Some("12".into()),
                rotation: Some(2),
            },
        },
    };
    let drag_json = serde_json::to_string(&drag).unwrap();
    assert_eq!(
        drag_json,
        r#"{"type":"event","event":{"type":"presence","presence":{"actor":"a1b2c3d4","label":"player-a1b2","cell":{"x":5,"y":6},"kind":"drag","def_id":"12","rotation":2}}}"#
    );
    assert_eq!(serde_json::from_str::<ServerMessage>(&drag_json).unwrap(), drag);
}

#[test]
fn joined_and_left_frames_match_contract_shapes() {
    assert_eq!(
        serde_json::to_string(&ServerMessage::Event {
            event: Event::PlayerJoined {
                actor: "a1b2c3d4".into(),
                label: "player-a1b2".into(),
            }
        })
        .unwrap(),
        r#"{"type":"event","event":{"type":"player_joined","actor":"a1b2c3d4","label":"player-a1b2"}}"#
    );
    assert_eq!(
        serde_json::to_string(&ServerMessage::Event {
            event: Event::PlayerLeft { actor: "a1b2c3d4".into() }
        })
        .unwrap(),
        r#"{"type":"event","event":{"type":"player_left","actor":"a1b2c3d4"}}"#
    );
}

#[test]
fn command_result_frames_match_contract_shapes() {
    let accepted = ServerMessage::CommandResult {
        result: CommandResult::Accepted {
            command_id: "c1".into(),
            building_id: "7".into(),
            owner: "a1b2c3d4".into(),
        },
    };
    assert_eq!(
        serde_json::to_string(&accepted).unwrap(),
        r#"{"type":"command_result","result":{"result":"accepted","command_id":"c1","building_id":"7","owner":"a1b2c3d4"}}"#
    );

    let cleared = ServerMessage::CommandResult {
        result: CommandResult::ClearedBuildings {
            command_id: "c3".into(),
            removed: 3,
        },
    };
    assert_eq!(
        serde_json::to_string(&cleared).unwrap(),
        r#"{"type":"command_result","result":{"result":"cleared_buildings","command_id":"c3","removed":3}}"#
    );

    let rejected = ServerMessage::CommandResult {
        result: CommandResult::Rejected {
            command_id: "c1".into(),
            code: RejectCode::Overlap,
            message: "footprint overlaps building 3".into(),
            conflicting_building_id: Some("3".into()),
        },
    };
    assert_eq!(
        serde_json::to_string(&rejected).unwrap(),
        r#"{"type":"command_result","result":{"result":"rejected","command_id":"c1","code":"overlap","message":"footprint overlaps building 3","conflicting_building_id":"3"}}"#
    );

    // No conflict: the field is absent, never null.
    let rejected = ServerMessage::CommandResult {
        result: CommandResult::Rejected {
            command_id: "c1".into(),
            code: RejectCode::BadRequest,
            message: "unrecognized command".into(),
            conflicting_building_id: None,
        },
    };
    assert_eq!(
        serde_json::to_string(&rejected).unwrap(),
        r#"{"type":"command_result","result":{"result":"rejected","command_id":"c1","code":"bad_request","message":"unrecognized command"}}"#
    );
}

#[test]
fn reject_codes_serialize_snake_case() {
    for (code, wire) in [
        (RejectCode::Overlap, "overlap"),
        (RejectCode::OutOfBounds, "out_of_bounds"),
        (RejectCode::UnknownAsset, "unknown_asset"),
        (RejectCode::UnknownBuilding, "unknown_building"),
        (
            RejectCode::DuplicateIdentityConflict,
            "duplicate_identity_conflict",
        ),
        (RejectCode::BadRequest, "bad_request"),
    ] {
        assert_eq!(
            serde_json::to_string(&code).unwrap(),
            format!(r#""{wire}""#)
        );
    }
}

/// The typed enums are the "known archetype/style" rule: unknown wire values
/// fail deserialization at the boundary.
#[test]
fn unknown_enum_values_are_rejected() {
    let bad = r##"{"name":"x","archetype":"factory","footprint_w":1,"footprint_h":1,"floors":1,"base_color":"#000000","roof_color":"#000000","window_style":"grid","seed":1}"##;
    assert!(serde_json::from_str::<Recipe>(bad).is_err());
    let bad = r##"{"name":"x","archetype":"house","footprint_w":1,"footprint_h":1,"floors":1,"base_color":"#000000","roof_color":"#000000","window_style":"venetian","seed":1}"##;
    assert!(serde_json::from_str::<Recipe>(bad).is_err());
}
