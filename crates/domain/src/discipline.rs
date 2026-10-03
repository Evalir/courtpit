//! Singles, doubles and mixed doubles.

use serde::{Deserialize, Serialize};

/// What kind of tennis is played. Ranked separately (see `mixed_pooling` in scoring).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Discipline {
    /// One player per side.
    Singles,
    /// Two players per side.
    Doubles,
    /// Two players per side, one of each gender.
    Mixed,
}

impl Discipline {
    /// Every discipline.
    pub const ALL: [Self; 3] = [Self::Singles, Self::Doubles, Self::Mixed];

    /// Players on each side of a match (and in each league/tournament entry).
    #[must_use]
    pub const fn players_per_side(self) -> usize {
        match self {
            Self::Singles => 1,
            Self::Doubles | Self::Mixed => 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_sizes() {
        assert_eq!(Discipline::Singles.players_per_side(), 1);
        assert_eq!(Discipline::Doubles.players_per_side(), 2);
        assert_eq!(Discipline::Mixed.players_per_side(), 2);
    }

    #[test]
    fn json() {
        assert_eq!(
            serde_json::to_string(&Discipline::Mixed).unwrap(),
            "\"mixed\""
        );
        assert_eq!(
            serde_json::from_str::<Discipline>("\"doubles\"").unwrap(),
            Discipline::Doubles
        );
    }
}
