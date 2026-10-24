//! Stable domain identities. Array slots, entity indices, and connection ids
//! are never durable identity.

/// A multiplayer actor. The value is the seat's `/api/seed` UUID: stable
/// across reconnects (a player), whereas connections are ephemeral. Kept as
/// a newtype so the domain can never confuse it with a `WireId`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct ActorId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildingId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetId(pub u64);
