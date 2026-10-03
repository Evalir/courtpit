//! Newtype ids, so a player id can't be passed where an entry id is expected.
//!
//! The server stores plain `uuid`s; it wraps them at the domain boundary.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($(#[$doc:meta] $name:ident),* $(,)?) => {$(
        #[$doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl From<Uuid> for $name {
            fn from(id: Uuid) -> Self {
                Self(id)
            }
        }

        impl From<$name> for Uuid {
            fn from(id: $name) -> Self {
                id.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    )*};
}

id_type! {
    /// A community (tenant).
    CommunityId,
    /// A player: a user's membership in one community.
    PlayerId,
    /// A match.
    MatchId,
    /// A league (one season).
    LeagueId,
    /// A league division ("box").
    DivisionId,
    /// A league or tournament entry (one player, or a doubles pair).
    EntryId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_json_and_conversions() {
        let raw = Uuid::nil();
        let id = EntryId::from(raw);
        assert_eq!(Uuid::from(id), raw);
        assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{raw}\""));
        assert_eq!(id.to_string(), raw.to_string());
    }
}
