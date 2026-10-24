//! Authoritative asset catalogue: validated recipes and derived gameplay stats.

use std::collections::BTreeMap;

use crate::id::AssetId;
use city_protocol::{Archetype, DerivedStats, Recipe};

/// Server-derived gameplay definition. Clients may propose recipes; only the
/// server's derived capacity/cost are authoritative.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovedDefinition {
    pub recipe: Recipe,
    pub derived: DerivedStats,
}

/// Capacity density per cell·floor in hundredths of a person, so the
/// derivation stays in exact integer math. Contracts table:
/// house 0.15, shop 0.10, office 0.30, tower 0.40.
fn density_hundredths(archetype: Archetype) -> u64 {
    match archetype {
        Archetype::House => 15,
        Archetype::Shop => 10,
        Archetype::Office => 30,
        Archetype::Tower => 40,
    }
}

/// Credits per cell·floor (contracts table).
fn cost_per_cell_floor(archetype: Archetype) -> u64 {
    match archetype {
        Archetype::House => 120,
        Archetype::Shop => 180,
        Archetype::Office => 260,
        Archetype::Tower => 340,
    }
}

/// Derives the authoritative stats: `capacity = floor(w·h·floors·density)`,
/// `cost_credits = w·h·floors·cost_per_cell`, all in integer math. The density
/// is scaled by 100, so capacity is `cell_floors * density_hundredths / 100`.
/// Callers must have validated the recipe; the arithmetic only overflows far
/// beyond the legal 8×8×40 maximum.
fn derive_stats(recipe: &Recipe) -> DerivedStats {
    let cell_floors =
        u64::from(recipe.footprint_w) * u64::from(recipe.footprint_h) * u64::from(recipe.floors);
    DerivedStats {
        capacity: (cell_floors * density_hundredths(recipe.archetype) / 100) as u32,
        cost_credits: cell_floors * cost_per_cell_floor(recipe.archetype),
    }
}

#[derive(Debug, Default)]
pub struct AssetCatalogue {
    definitions: BTreeMap<AssetId, ApprovedDefinition>,
}

impl AssetCatalogue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores a definition under its durable id (replaces an existing entry).
    /// Publishing always allocates a fresh id, so stored recipes are immutable.
    pub fn insert(&mut self, id: AssetId, approved: ApprovedDefinition) {
        self.definitions.insert(id, approved);
    }

    pub fn get(&self, id: AssetId) -> Option<&ApprovedDefinition> {
        self.definitions.get(&id)
    }

    /// Removes and returns a definition. The placed-buildings check belongs
    /// to the caller (it owns the world), so removal is total here.
    pub fn remove(&mut self, id: AssetId) -> Option<ApprovedDefinition> {
        self.definitions.remove(&id)
    }

    /// All definitions in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = (AssetId, &ApprovedDefinition)> + '_ {
        self.definitions.iter().map(|(id, def)| (*id, def))
    }
}

/// Validates a client-proposed recipe against every rule and derives the
/// authoritative stats table. Returns ALL violations, not just the first.
///
/// Archetype and window style need no checks here: they are typed enums, so an
/// unknown wire value fails deserialization at the boundary and can never
/// reach this function as a `Recipe`.
pub fn validate_recipe(recipe: &Recipe) -> Result<ApprovedDefinition, Vec<String>> {
    let mut errors = Vec::new();

    let name_chars = recipe.name.chars().count();
    if !(1..=64).contains(&name_chars) {
        errors.push(format!("name must be 1..=64 characters (got {name_chars})"));
    }
    for (field, dim) in [
        ("footprint_w", recipe.footprint_w),
        ("footprint_h", recipe.footprint_h),
    ] {
        if !(1..=8).contains(&dim) {
            errors.push(format!("{field} must be within 1..=8 (got {dim})"));
        }
    }
    if !(1..=40).contains(&recipe.floors) {
        errors.push(format!(
            "floors must be within 1..=40 (got {})",
            recipe.floors
        ));
    }
    for (field, color) in [
        ("base_color", &recipe.base_color),
        ("roof_color", &recipe.roof_color),
    ] {
        if !is_hex_color(color) {
            errors.push(format!("{field} must match #rrggbb (got {color:?})"));
        }
    }

    // Authored-geometry rules. Parts are metres relative to the footprint's
    // min corner; the +2 m allowance above the floor height covers rooftop
    // clutter (AC units, tanks, antennas).
    const MAX_PARTS: usize = 64;
    const PART_MAX_DIM: f32 = 24.0;
    let max_part_top = recipe.floors as f32 * 3.0 + 2.0;
    if recipe.parts.len() > MAX_PARTS {
        errors.push(format!(
            "parts must have at most {MAX_PARTS} entries (got {})",
            recipe.parts.len()
        ));
    }
    for (index, part) in recipe.parts.iter().enumerate() {
        let label = |what: String| format!("parts[{index}]: {what}");
        if !(part.w > 0.0 && part.h > 0.0 && part.d > 0.0) {
            errors.push(label("dimensions must be positive".to_owned()));
        }
        if part.w > PART_MAX_DIM || part.h > PART_MAX_DIM || part.d > PART_MAX_DIM {
            errors.push(label(format!("dimensions must be within {PART_MAX_DIM} m")));
        }
        if part.x < 0.0 || part.x + part.w > recipe.footprint_w as f32 + 1e-3 {
            errors.push(label("x extent reaches outside the footprint".to_owned()));
        }
        if part.z < 0.0 || part.z + part.d > recipe.footprint_h as f32 + 1e-3 {
            errors.push(label("z extent reaches outside the footprint".to_owned()));
        }
        if part.y < 0.0 || part.y + part.h > max_part_top + 1e-3 {
            errors.push(label(format!(
                "y extent reaches above the {} m build height",
                max_part_top as u32
            )));
        }
        if !is_hex_color(&part.color) {
            errors.push(label(format!(
                "color must match #rrggbb (got {:?})",
                part.color
            )));
        }
        if part.kind.len() > 24 {
            errors.push(label("kind must be at most 24 characters".to_owned()));
        }
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(ApprovedDefinition {
        recipe: recipe.clone(),
        derived: derive_stats(recipe),
    })
}

/// `^#[0-9a-fA-F]{6}$` without a regex dependency.
fn is_hex_color(color: &str) -> bool {
    let bytes = color.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use city_protocol::WindowStyle;

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

    /// Seed catalogue from docs/contracts/multiplayer-stream.md with values derived
    /// exactly from the contracts table:
    /// capacity = floor(w·h·floors·density), cost = w·h·floors·cost_per_cell.
    #[test]
    fn seed_recipes_derive_exact_table_values() {
        // house 3×2×3 = 18 cell·floors → floor(18 × 0.15) = 2; 18 × 120 = 2160
        let approved = validate_recipe(&cedar()).unwrap();
        assert_eq!(
            approved.derived,
            DerivedStats {
                capacity: 2,
                cost_credits: 2160
            }
        );
        assert_eq!(approved.recipe, cedar());

        let harbor = Recipe {
            name: "Harbor Point Office".into(),
            archetype: Archetype::Office,
            footprint_w: 3,
            footprint_h: 3,
            floors: 8,
            base_color: "#7f9bb3".into(),
            roof_color: "#2f2f38".into(),
            window_style: WindowStyle::Stripes,
            seed: 22,
            parts: Vec::new(),
        };
        // office 3×3×8 = 72 cell·floors → floor(72 × 0.30) = 21; 72 × 260 = 18720
        let approved = validate_recipe(&harbor).unwrap();
        assert_eq!(
            approved.derived,
            DerivedStats {
                capacity: 21,
                cost_credits: 18720
            }
        );

        let meridian = Recipe {
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
        };
        // tower 2×2×24 = 96 cell·floors → floor(96 × 0.40) = 38; 96 × 340 = 32640
        let approved = validate_recipe(&meridian).unwrap();
        assert_eq!(
            approved.derived,
            DerivedStats {
                capacity: 38,
                cost_credits: 32640
            }
        );
    }

    #[test]
    fn every_rule_violation_is_reported_together() {
        let bad = Recipe {
            name: String::new(),
            archetype: Archetype::Shop,
            footprint_w: 0,
            footprint_h: 9,
            floors: 41,
            base_color: "red".into(),
            roof_color: "#12345g".into(),
            window_style: WindowStyle::Stripes,
            seed: 7,
            parts: Vec::new(),
        };
        let errors = validate_recipe(&bad).unwrap_err();
        assert_eq!(errors.len(), 6, "expected all violations, got {errors:?}");
        for needle in [
            "name",
            "footprint_w",
            "footprint_h",
            "floors",
            "base_color",
            "roof_color",
        ] {
            assert!(
                errors.iter().any(|e| e.contains(needle)),
                "missing {needle} violation in {errors:?}"
            );
        }
    }

    fn part(x: f32, y: f32, z: f32, w: f32, h: f32, d: f32) -> city_protocol::AssetPart {
        city_protocol::AssetPart {
            kind: "box".into(),
            x,
            y,
            z,
            w,
            h,
            d,
            color: "#c96f4a".into(),
        }
    }

    #[test]
    fn valid_parts_pass_and_keep_field_derivation() {
        let mut recipe = cedar();
        recipe.parts = vec![part(0.0, 0.0, 0.0, 3.0, 9.0, 2.0)];
        let approved = validate_recipe(&recipe).unwrap();
        // Stats still derive from the recipe fields, not the parts.
        assert_eq!(approved.derived.capacity, 2);
    }

    #[test]
    fn parts_violations_report_geometry_and_color() {
        let mut recipe = cedar();
        recipe.parts = vec![
            part(-0.5, 0.0, 0.0, 1.0, 1.0, 1.0),   // x outside footprint
            part(0.0, 0.0, 0.0, 4.0, 1.0, 1.0),    // x extent past footprint_w=3
            part(0.0, 12.0, 0.0, 1.0, 1.0, 1.0),   // above 3 floors × 3 m + 2 m
            part(0.0, 0.0, 0.0, 0.0, 1.0, 1.0),    // non-positive dimension
            city_protocol::AssetPart {
                color: "red".into(),
                ..part(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)
            },
        ];
        let errors = validate_recipe(&recipe).unwrap_err();
        assert_eq!(errors.len(), 5, "got {errors:?}");
        for needle in ["parts[0]", "parts[1]", "parts[2]", "parts[3]", "parts[4]"] {
            assert!(errors.iter().any(|e| e.contains(needle)), "missing {needle} in {errors:?}");
        }
    }

    #[test]
    fn rule_boundaries_accept_and_reject() {
        let mut recipe = cedar();

        recipe.name = "x".repeat(1);
        assert!(validate_recipe(&recipe).is_ok());
        recipe.name = "x".repeat(64);
        assert!(validate_recipe(&recipe).is_ok());
        recipe.name = "x".repeat(65);
        assert!(validate_recipe(&recipe).is_err());

        recipe = cedar();
        recipe.footprint_w = 1;
        recipe.footprint_h = 8;
        assert!(validate_recipe(&recipe).is_ok());
        recipe.footprint_w = 9;
        assert!(validate_recipe(&recipe).is_err());

        recipe = cedar();
        recipe.floors = 1;
        assert!(validate_recipe(&recipe).is_ok());
        recipe.floors = 40;
        assert!(validate_recipe(&recipe).is_ok());
        recipe.floors = 0;
        assert!(validate_recipe(&recipe).is_err());

        recipe = cedar();
        recipe.roof_color = "#ABCDEF".into();
        assert!(validate_recipe(&recipe).is_ok(), "uppercase hex is legal");
        recipe.roof_color = "#12345".into();
        assert!(validate_recipe(&recipe).is_err(), "too short");
        recipe.roof_color = "#1234567".into();
        assert!(validate_recipe(&recipe).is_err(), "too long");
        recipe.roof_color = "123456".into();
        assert!(validate_recipe(&recipe).is_err(), "missing #");
        recipe.roof_color = "#12345z".into();
        assert!(validate_recipe(&recipe).is_err(), "non-hex digit");
    }

    #[test]
    fn catalogue_stores_looks_up_and_iterates_in_id_order() {
        let mut catalogue = AssetCatalogue::new();
        let approved = validate_recipe(&cedar()).unwrap();
        catalogue.insert(AssetId(7), approved.clone());
        assert_eq!(catalogue.get(AssetId(7)), Some(&approved));
        assert_eq!(catalogue.get(AssetId(8)), None);

        catalogue.insert(AssetId(2), approved.clone());
        let ids: Vec<u64> = catalogue.iter().map(|(id, _)| id.0).collect();
        assert_eq!(ids, vec![2, 7]);
    }
}
