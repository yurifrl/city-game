//! A seat's read-only copy of the world, rebuilt from the same wire events
//! every other client receives, and its conversion to renderable boxes.

use std::collections::{BTreeMap, HashMap};

use city_protocol as wire;
use city_render::Box3;

/// Metres per storey. One grid cell is one metre square.
const FLOOR_HEIGHT: f32 = 3.0;
/// Roof slab thickness for procedural (part-less) buildings.
const ROOF_THICKNESS: f32 = 0.3;
const GHOST_COLOR: [f32; 3] = [0.95, 0.85, 0.30];

pub(super) struct Replica {
    pub(super) grid: [u32; 2],
    me: wire::ActorId,
    buildings: BTreeMap<wire::WireId, wire::BuildingDto>,
    definitions: BTreeMap<u64, wire::AssetDefinition>,
    presences: HashMap<wire::ActorId, wire::PresenceState>,
}

impl Replica {
    pub(super) fn new(
        me: wire::ActorId,
        world: wire::WorldDto,
        presences: Vec<wire::PresenceState>,
    ) -> Self {
        let mut replica = Replica {
            grid: [world.grid.width as u32, world.grid.height as u32],
            me,
            buildings: world
                .buildings
                .into_iter()
                .map(|b| (b.id.clone(), b))
                .collect(),
            definitions: BTreeMap::new(),
            presences: presences.into_iter().map(|p| (p.actor.clone(), p)).collect(),
        };
        for definition in world.definitions {
            replica.insert_definition(definition);
        }
        replica
    }

    fn insert_definition(&mut self, definition: wire::AssetDefinition) {
        // Wire ids are decimal strings the server itself produced.
        if let Ok(id) = definition.id.parse::<u64>() {
            self.definitions.insert(id, definition);
        }
    }

    /// Applies one authoritative event. Returns true when the picture changed.
    pub(super) fn apply(&mut self, event: wire::Event) -> bool {
        match event {
            wire::Event::BuildingPlaced { building } => {
                self.buildings.insert(building.id.clone(), building);
            }
            wire::Event::BuildingDemolished { building_id, .. } => {
                self.buildings.remove(&building_id);
            }
            wire::Event::BuildingsCleared { .. } => self.buildings.clear(),
            wire::Event::DefinitionPublished { definition } => self.insert_definition(definition),
            wire::Event::DefinitionRemoved { def_id } => {
                if let Ok(id) = def_id.parse::<u64>() {
                    self.definitions.remove(&id);
                }
            }
            wire::Event::Presence { presence } => {
                if presence.actor == self.me {
                    return false;
                }
                self.presences.insert(presence.actor.clone(), presence);
            }
            wire::Event::PlayerLeft { actor } => {
                self.presences.remove(&actor);
            }
            wire::Event::PlayerJoined { .. } => return false,
        }
        true
    }

    /// Definition ids in ascending order; the seat maps digit keys onto them.
    pub(super) fn definition_ids(&self) -> Vec<u64> {
        self.definitions.keys().copied().collect()
    }

    pub(super) fn definition_name(&self, def_id: u64) -> Option<&str> {
        self.definitions
            .get(&def_id)
            .map(|d| d.recipe.name.as_str())
    }

    /// Id of the building covering `cell`, if any.
    pub(super) fn building_at(&self, cell: [i32; 2]) -> Option<&wire::WireId> {
        self.buildings.values().find_map(|b| {
            let def = self.definitions.get(&b.def_id.parse::<u64>().ok()?)?;
            let (w, h) = rotated_footprint(&def.recipe, b.rotation);
            let inside = cell[0] >= b.cell.x
                && cell[0] < b.cell.x + w as i32
                && cell[1] >= b.cell.y
                && cell[1] < b.cell.y + h as i32;
            inside.then_some(&b.id)
        })
    }

    /// Every box to draw: buildings, then other players' cursors and ghosts,
    /// then this seat's own ghost (`own_ghost` = definition, cell, rotation).
    pub(super) fn boxes(&self, own_ghost: Option<(u64, [i32; 2], u8)>) -> Vec<Box3> {
        let mut boxes = Vec::with_capacity(self.buildings.len() * 2);
        for building in self.buildings.values() {
            let Some(def) = building
                .def_id
                .parse::<u64>()
                .ok()
                .and_then(|id| self.definitions.get(&id))
            else {
                continue;
            };
            building_boxes(
                &def.recipe,
                [building.cell.x as f32, building.cell.y as f32],
                building.rotation,
                &mut boxes,
            );
        }
        for presence in self.presences.values() {
            let color = actor_color(&presence.actor);
            boxes.push(Box3 {
                min: [presence.cell.x as f32 + 0.2, presence.cell.y as f32 + 0.2],
                size: [0.6, 0.6],
                base: 0.0,
                height: 0.6,
                color,
            });
            if let Some(def) = presence
                .def_id
                .as_ref()
                .and_then(|id| id.parse::<u64>().ok())
                .and_then(|id| self.definitions.get(&id))
            {
                ghost_box(
                    &def.recipe,
                    [presence.cell.x, presence.cell.y],
                    presence.rotation.unwrap_or(0),
                    color,
                    &mut boxes,
                );
            }
        }
        if let Some((def_id, cell, rotation)) = own_ghost {
            if let Some(def) = self.definitions.get(&def_id) {
                ghost_box(&def.recipe, cell, rotation, GHOST_COLOR, &mut boxes);
            }
        }
        boxes
    }
}

fn rotated_footprint(recipe: &wire::Recipe, rotation: u8) -> (u32, u32) {
    if rotation % 2 == 1 {
        (recipe.footprint_h, recipe.footprint_w)
    } else {
        (recipe.footprint_w, recipe.footprint_h)
    }
}

fn ghost_box(recipe: &wire::Recipe, cell: [i32; 2], rotation: u8, color: [f32; 3], out: &mut Vec<Box3>) {
    let (w, h) = rotated_footprint(recipe, rotation);
    out.push(Box3 {
        min: [cell[0] as f32, cell[1] as f32],
        size: [w as f32, h as f32],
        base: 0.0,
        height: 0.4,
        color,
    });
}

/// Parses `#rrggbb` into 0..=1 channels. Recipes are validated server-side
/// at publish time, so a malformed colour here means a corrupted catalogue.
fn parse_color(hex: &str) -> [f32; 3] {
    let channel = |start: usize| {
        hex.get(start..start + 2)
            .and_then(|digits| u8::from_str_radix(digits, 16).ok())
            .map(|value| f32::from(value) / 255.0)
            // Magenta marks an unparseable colour visibly instead of hiding it.
            .unwrap_or(if start == 3 { 0.0 } else { 1.0 })
    };
    [channel(1), channel(3), channel(5)]
}

fn actor_color(actor: &str) -> [f32; 3] {
    let hash = actor
        .bytes()
        .fold(2166136261u32, |acc, byte| (acc ^ u32::from(byte)).wrapping_mul(16777619));
    let channel = |shift: u32| 0.35 + 0.65 * (((hash >> shift) & 0xff) as f32 / 255.0);
    [channel(0), channel(8), channel(16)]
}

/// Appends the boxes of one placed building. `origin` is the footprint's
/// minimum cell after rotation. Rotation is clockwise quarter turns seen
/// from above.
fn building_boxes(recipe: &wire::Recipe, origin: [f32; 2], rotation: u8, out: &mut Vec<Box3>) {
    if recipe.parts.is_empty() {
        let (w, h) = rotated_footprint(recipe, rotation);
        let total = recipe.floors as f32 * FLOOR_HEIGHT;
        out.push(Box3 {
            min: origin,
            size: [w as f32, h as f32],
            base: 0.0,
            height: total - ROOF_THICKNESS,
            color: parse_color(&recipe.base_color),
        });
        out.push(Box3 {
            min: origin,
            size: [w as f32, h as f32],
            base: total - ROOF_THICKNESS,
            height: ROOF_THICKNESS,
            color: parse_color(&recipe.roof_color),
        });
        return;
    }
    for part in &recipe.parts {
        // Footprint-relative rectangle, rotated one quarter turn at a time.
        let (mut x, mut z, mut w, mut d) = (part.x, part.z, part.w, part.d);
        let (mut fw, mut fh) = (recipe.footprint_w as f32, recipe.footprint_h as f32);
        for _ in 0..rotation % 4 {
            (x, z, w, d) = (fh - (z + d), x, d, w);
            (fw, fh) = (fh, fw);
        }
        out.push(Box3 {
            min: [origin[0] + x, origin[1] + z],
            size: [w, d],
            base: part.y,
            height: part.h,
            color: parse_color(&part.color),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(parts: Vec<wire::AssetPart>) -> wire::Recipe {
        wire::Recipe {
            name: "t".into(),
            archetype: wire::Archetype::House,
            footprint_w: 4,
            footprint_h: 2,
            floors: 2,
            base_color: "#ff0000".into(),
            roof_color: "#00ff00".into(),
            window_style: wire::WindowStyle::Grid,
            seed: 0,
            parts,
        }
    }

    #[test]
    fn colours_parse_and_bad_colours_are_visibly_magenta() {
        assert_eq!(parse_color("#ff8000"), [1.0, 128.0 / 255.0, 0.0]);
        assert_eq!(parse_color("nope"), [1.0, 0.0, 1.0]);
    }

    #[test]
    fn procedural_building_is_a_body_plus_roof_covering_the_rotated_footprint() {
        let mut boxes = Vec::new();
        building_boxes(&recipe(vec![]), [10.0, 20.0], 1, &mut boxes);
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[0].size, [2.0, 4.0]);
        assert_eq!(boxes[1].color, [0.0, 1.0, 0.0]);
        assert!((boxes[1].base + boxes[1].height - 6.0).abs() < 1e-6);
    }

    #[test]
    fn a_part_keeps_its_place_in_the_footprint_through_quarter_turns() {
        // A 1x1 part in the footprint's min corner of a 4x2 footprint.
        let part = wire::AssetPart {
            kind: "box".into(),
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
            h: 1.0,
            d: 1.0,
            color: "#ffffff".into(),
        };
        let recipe = recipe(vec![part]);
        let mut boxes = Vec::new();
        building_boxes(&recipe, [0.0, 0.0], 1, &mut boxes);
        // After one clockwise quarter turn the footprint is 2 wide, 4 tall and
        // the min corner moves to the top-right: x = 2 - 1, z = 0.
        assert_eq!(boxes[0].min, [1.0, 0.0]);
        let mut full_turn = Vec::new();
        building_boxes(&recipe, [0.0, 0.0], 3, &mut full_turn);
        assert_eq!(full_turn[0].min, [0.0, 3.0]);
    }
}
