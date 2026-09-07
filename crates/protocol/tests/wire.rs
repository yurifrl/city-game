//! Wire-shape tests: the JSON frames defined in docs/contracts/milestone-1.md
//! are normative, so every frame here must roundtrip byte for byte.

use city_protocol::{
    Archetype, AssetDefinition, BuildingDto, Cell, ClientMessage, Command, CommandResult, DerivedStats,
    Event, GridDto, Recipe, RejectCode, ServerMessage, WindowStyle, WorldDto,
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

/// Domain ids cross the wire as decimal strings so they stay safe above 2^53
/// in JavaScript.
#[test]
fn wire_ids_stay_decimal_strings() {
    let dto = BuildingDto {
        id: "9007199254740993".into(),
        def_id: "1".into(),
        revision: 1,
        cell: Cell { x: 1, y: 2 },
        rotation: 0,
        derived: DerivedStats { capacity: 2, cost_credits: 2160 },
    };
    let json = serde_json::to_string(&dto).unwrap();
    assert!(json.contains(r#""id":"9007199254740993""#), "got: {json}");
    assert!(json.contains(r#""def_id":"1""#), "got: {json}");
    assert_eq!(serde_json::from_str::<BuildingDto>(&json).unwrap(), dto);
}

#[test]
fn hello_frame_matches_contract_shape() {
    let world = WorldDto {
        grid: GridDto { width: 96, height: 96 },
        tick: 0,
        buildings: vec![],
        definitions: vec![],
    };
    let hello: ServerMessage = serde_json::from_str(
        r#"{"type":"hello","actor":"owner","world":{"grid":{"width":96,"height":96},"tick":0,"buildings":[],"definitions":[]}}"#,
    )
    .unwrap();
    assert_eq!(
        hello,
        ServerMessage::Hello {
            actor: "owner".into(),
            world,
        }
    );
    assert_eq!(
        serde_json::to_string(&hello).unwrap(),
        r#"{"type":"hello","actor":"owner","world":{"grid":{"width":96,"height":96},"tick":0,"buildings":[],"definitions":[]}}"#
    );
}

#[test]
fn event_frames_match_contract_shapes() {
    let building = BuildingDto {
        id: "1".into(),
        def_id: "1".into(),
        revision: 1,
        cell: Cell { x: 10, y: 12 },
        rotation: 0,
        derived: DerivedStats { capacity: 2, cost_credits: 2160 },
    };
    assert_eq!(
        serde_json::to_string(&ServerMessage::Event {
            event: Event::BuildingPlaced { building }
        })
        .unwrap(),
        concat!(
            r#"{"type":"event","event":{"type":"building_placed","#,
            r#""building":{"id":"1","def_id":"1","revision":1,"cell":{"x":10,"y":12},"rotation":0,"derived":{"capacity":2,"cost_credits":2160}}}}"#
        )
    );
    assert_eq!(
        serde_json::to_string(&ServerMessage::Event {
            event: Event::BuildingDemolished {
                building_id: "7".into()
            }
        })
        .unwrap(),
        r#"{"type":"event","event":{"type":"building_demolished","building_id":"7"}}"#
    );

    let published = ServerMessage::Event {
        event: Event::DefinitionPublished {
            definition: AssetDefinition {
                id: "1".into(),
                revision: 1,
                recipe: cedar(),
                derived: DerivedStats { capacity: 2, cost_credits: 2160 },
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
                    },
                    "derived": { "capacity": 2, "cost_credits": 2160 },
                },
            },
        })
    );
}

#[test]
fn command_result_frames_match_contract_shapes() {
    let accepted = ServerMessage::CommandResult {
        result: CommandResult::Accepted {
            command_id: "c1".into(),
            building_id: "7".into(),
        },
    };
    assert_eq!(
        serde_json::to_string(&accepted).unwrap(),
        r#"{"type":"command_result","result":{"result":"accepted","command_id":"c1","building_id":"7"}}"#
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
        (RejectCode::DuplicateIdentityConflict, "duplicate_identity_conflict"),
        (RejectCode::BadRequest, "bad_request"),
    ] {
        assert_eq!(serde_json::to_string(&code).unwrap(), format!(r#""{wire}""#));
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
