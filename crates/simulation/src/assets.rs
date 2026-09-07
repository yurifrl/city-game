//! Authoritative asset catalogue: validated recipes and derived gameplay stats.

use crate::id::AssetId;
use city_protocol::{DerivedStats, Recipe};

/// Server-derived gameplay definition. Clients may propose recipes; only the
/// server's derived capacity/cost are authoritative.
#[derive(Debug, Clone, PartialEq)]
pub struct ApprovedDefinition {
    pub recipe: Recipe,
    pub derived: DerivedStats,
}

#[derive(Debug, Default)]
pub struct AssetCatalogue {
    // m1-01 owns the representation.
}

impl AssetCatalogue {
    pub fn new() -> Self {
        todo!("m1-01")
    }

    pub fn insert(&mut self, id: AssetId, approved: ApprovedDefinition) {
        todo!("m1-01")
    }

    pub fn get(&self, id: AssetId) -> Option<&ApprovedDefinition> {
        todo!("m1-01")
    }

    pub fn iter(&self) -> impl Iterator<Item = (AssetId, &ApprovedDefinition)> + '_ {
        std::iter::from_fn(|| todo!("m1-01"))
    }
}

/// Validates a client-proposed recipe against every rule and derives the
/// authoritative stats table. Returns ALL violations, not just the first.
pub fn validate_recipe(recipe: &Recipe) -> Result<ApprovedDefinition, Vec<String>> {
    todo!("m1-01")
}
