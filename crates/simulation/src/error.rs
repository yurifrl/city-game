//! Explicit rejection reasons. Callers translate these into wire codes and
//! player-facing explanations; the domain never hides a failure.

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Reject {
    #[error("footprint overlaps building {conflicting:?}")]
    Overlap { conflicting: crate::id::BuildingId },
    #[error("footprint extends outside the {0}x{1} grid")]
    OutOfBounds(i32, i32),
    #[error("unknown asset definition {0:?}")]
    UnknownAsset(crate::id::AssetId),
    #[error("unknown building {0:?}")]
    UnknownBuilding(crate::id::BuildingId),
}
