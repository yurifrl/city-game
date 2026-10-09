//! Authoritative city domain. No sockets, databases, clocks, or rendering here.
//! The public surface in this file is normative (docs/contracts/milestone-1.md):
//! the server compiles against these signatures.

pub mod assets;
pub mod construction;
pub mod error;
pub mod id;
pub mod save;
pub mod world;

pub use assets::{ApprovedDefinition, AssetCatalogue};
pub use construction::{
    apply_demolition, apply_placement, footprint, validate_placement, Placement,
};
pub use error::Reject;
pub use id::{AssetId, BuildingId};
pub use save::{from_save, to_save, SaveAsset, SaveBuilding, WorldSave};
pub use world::{Building, Cell, WorldState, GRID_HEIGHT, GRID_WIDTH};
