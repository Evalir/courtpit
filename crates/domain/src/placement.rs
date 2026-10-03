//! Division ("box") placement for a league season.
//!
//! Confirmed entries are ordered by level and cut into boxes of `min_size..=max_size`
//! (default 6–8), tier 1 at the top. Returning entries honour last season's promotion or
//! relegation; newcomers are slotted by UTR. The result is deterministic for a given input.

use std::collections::HashMap;
use std::hash::Hash;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Box size limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct BoxSize {
    /// Smallest preferred box size.
    pub min_size: usize,
    /// Largest preferred box size.
    pub max_size: usize,
}

impl Default for BoxSize {
    fn default() -> Self {
        Self {
            min_size: 6,
            max_size: 8,
        }
    }
}

/// Why a box size is unusable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PlacementError {
    /// The limits do not satisfy `2 <= min_size <= max_size`.
    #[error("box sizes must satisfy 2 <= min_size <= max_size")]
    BoxSize,
}

/// Where an entry finished last season.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Previous {
    /// Tier of the box they played in (1 = top).
    pub tier: u32,
    /// Promotion (`-1` tier number), relegation (`+1`) or neither (`0`).
    pub movement: i8,
}

/// An entry to place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seed<T> {
    /// Identifier of the entry.
    pub id: T,
    /// The entry's level; for doubles the server passes the pair's mean UTR.
    pub utr: Option<Decimal>,
    /// Last season's result, if the entry played one.
    pub previous: Option<Previous>,
}

/// One box of the placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed<T> {
    /// 1 = top box.
    pub tier: u32,
    /// Members, strongest first.
    pub entries: Vec<T>,
    /// Lowest UTR in the box (informational).
    pub utr_min: Option<Decimal>,
    /// Highest UTR in the box (informational).
    pub utr_max: Option<Decimal>,
}

/// Sizes of the boxes for `n` entries: balanced sizes (larger boxes first) for the box count
/// that strays least outside `min_size..=max_size`, counted in entries; ties go to fewer,
/// fuller boxes (more matches per player). So 13 entries make 7 + 6, 11 make 6 + 5, and 10
/// stay one box of 10.
pub fn box_sizes(n: usize, limits: BoxSize) -> Result<Vec<usize>, PlacementError> {
    if limits.min_size < 2 || limits.min_size > limits.max_size {
        return Err(PlacementError::BoxSize);
    }
    let sizes = |count: usize| -> Vec<usize> {
        let (base, extra) = (n / count, n % count);
        (0..count)
            .map(|idx| base + usize::from(idx < extra))
            .collect()
    };
    let stray = |sizes: &[usize]| -> usize {
        sizes
            .iter()
            .map(|&size| {
                limits.min_size.saturating_sub(size) + size.saturating_sub(limits.max_size)
            })
            .sum()
    };
    let best = (1..=n).min_by_key(|&count| (stray(&sizes(count)), count));
    Ok(best.map(sizes).unwrap_or_default())
}

/// Orders seeds strongest first: higher UTR first, unknown UTR last, then by id.
fn by_utr<T: Ord + Copy>(seeds: &mut [Seed<T>]) {
    seeds.sort_by(|x, y| {
        y.utr
            .is_some()
            .cmp(&x.utr.is_some())
            .then_with(|| y.utr.cmp(&x.utr))
            .then_with(|| x.id.cmp(&y.id))
    });
}

fn cut<T: Copy>(ordered: &[Seed<T>], sizes: &[usize]) -> Vec<Placed<T>> {
    let mut out = Vec::with_capacity(sizes.len());
    let mut rest = ordered;
    for (idx, &size) in sizes.iter().enumerate() {
        let (chunk, tail) = rest.split_at(size.min(rest.len()));
        rest = tail;
        let utrs = || chunk.iter().filter_map(|seed| seed.utr);
        out.push(Placed {
            tier: u32::try_from(idx + 1).unwrap_or(u32::MAX),
            entries: chunk.iter().map(|seed| seed.id).collect(),
            utr_min: utrs().min(),
            utr_max: utrs().max(),
        });
    }
    out
}

/// Places `seeds` into boxes.
///
/// 1. Every entry gets a UTR-only tier (as if nobody had history).
/// 2. Returning entries replace it with last season's tier moved by their promotion or
///    relegation (clamped to the available tiers).
/// 3. Entries are sorted by (that tier, UTR desc, id) and cut into boxes of
///    [`box_sizes`].
pub fn place<T: Ord + Copy + Hash>(
    seeds: &[Seed<T>],
    limits: BoxSize,
) -> Result<Vec<Placed<T>>, PlacementError> {
    let sizes = box_sizes(seeds.len(), limits)?;
    let mut ordered = seeds.to_vec();
    by_utr(&mut ordered);
    let utr_tier: HashMap<T, u32> = cut(&ordered, &sizes)
        .into_iter()
        .flat_map(|placed| placed.entries.into_iter().map(move |id| (id, placed.tier)))
        .collect();
    let tiers = i64::try_from(sizes.len()).unwrap_or(i64::MAX).max(1);
    let effective = |seed: &Seed<T>| -> i64 {
        seed.previous.map_or_else(
            || utr_tier.get(&seed.id).copied().map_or(tiers, i64::from),
            |prev| (i64::from(prev.tier) + i64::from(prev.movement)).clamp(1, tiers),
        )
    };
    // `ordered` is already UTR-sorted; a stable sort by tier keeps that order within a tier.
    ordered.sort_by_key(|seed| effective(seed));
    Ok(cut(&ordered, &sizes))
}

/// Fewest entries a season needs: in total, and in every box.
pub const MIN_ENTRIES: usize = 2;

/// Why a placement cannot be played as a season.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Unplayable {
    /// Fewer than [`MIN_ENTRIES`] entries in the whole league.
    #[error("Fewer than two entries were confirmed by the start date.")]
    TooFewEntries,
    /// The placement leaves a box with fewer than [`MIN_ENTRIES`] entries.
    #[error("The draw would have left a box with fewer than two entries.")]
    ThinBox,
}

/// Whether `entries` entries placed into `boxes` make a playable season: at least
/// [`MIN_ENTRIES`] entries overall and in every box (a round robin needs opponents).
pub fn playable<T>(entries: usize, boxes: &[Placed<T>]) -> Result<(), Unplayable> {
    if entries < MIN_ENTRIES {
        Err(Unplayable::TooFewEntries)
    } else if boxes
        .iter()
        .any(|placed| placed.entries.len() < MIN_ENTRIES)
    {
        Err(Unplayable::ThinBox)
    } else {
        Ok(())
    }
}

/// Promotion and relegation for a finished box, per finishing position (index 0 = 1st):
/// `-1` promoted (to a lower tier number), `+1` relegated, `0` stays. The top `up` entries go
/// up unless this is the top tier, the bottom `down` go down unless it is the bottom tier;
/// in small boxes both are capped at half the box so nobody is moved both ways.
#[must_use]
pub fn movements(box_len: usize, tier: u32, tiers: u32, up: usize, down: usize) -> Vec<i8> {
    let half = box_len / 2;
    let up = if tier > 1 { up.min(half) } else { 0 };
    let down = if tier < tiers { down.min(half) } else { 0 };
    (0..box_len)
        .map(|i| {
            if i < up {
                -1
            } else {
                i8::from(i >= box_len - down)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utr(tenths: i64) -> Option<Decimal> {
        Some(Decimal::new(tenths, 1))
    }

    fn seeds(utrs: &[i64]) -> Vec<Seed<u32>> {
        utrs.iter()
            .enumerate()
            .map(|(idx, &tenths)| Seed {
                id: u32::try_from(idx).unwrap(),
                utr: utr(tenths),
                previous: None,
            })
            .collect()
    }

    #[test]
    fn box_sizes_table() {
        let limits = BoxSize::default();
        for (n, want) in [
            (0, vec![]),
            (1, vec![1]),
            (5, vec![5]),
            (6, vec![6]),
            (8, vec![8]),
            (9, vec![9]),
            (10, vec![10]),
            (11, vec![6, 5]),
            (12, vec![6, 6]),
            (13, vec![7, 6]),
            (16, vec![8, 8]),
            (17, vec![9, 8]),
            (18, vec![6, 6, 6]),
            (25, vec![7, 6, 6, 6]),
        ] {
            assert_eq!(box_sizes(n, limits).unwrap(), want, "n={n}");
        }
        let small = BoxSize {
            min_size: 4,
            max_size: 6,
        };
        assert_eq!(box_sizes(7, small).unwrap(), vec![7]);
        assert_eq!(box_sizes(10, small).unwrap(), vec![5, 5]);
        for bad in [(1, 4), (5, 4)] {
            let limits = BoxSize {
                min_size: bad.0,
                max_size: bad.1,
            };
            assert_eq!(box_sizes(10, limits), Err(PlacementError::BoxSize));
        }
    }

    #[test]
    fn newcomers_are_placed_by_utr_then_id() {
        // 13 entries -> boxes of 7 and 6.
        let mut all_seeds = seeds(&[50, 80, 30, 80, 60, 20, 90, 40, 70, 10, 55, 65, 75]);
        all_seeds[5].utr = None;
        let boxes = place(&all_seeds, BoxSize::default()).unwrap();
        assert_eq!(boxes.len(), 2);
        assert_eq!(boxes[0].tier, 1);
        // 90, 80 (id 1), 80 (id 3), 75, 70, 65, 60
        assert_eq!(boxes[0].entries, vec![6, 1, 3, 12, 8, 11, 4]);
        // 55, 50, 40, 30, 10, unknown last
        assert_eq!(boxes[1].entries, vec![10, 0, 7, 2, 9, 5]);
        assert_eq!((boxes[0].utr_min, boxes[0].utr_max), (utr(60), utr(90)));
        assert_eq!((boxes[1].utr_min, boxes[1].utr_max), (utr(10), utr(55)));
        assert_eq!(
            boxes,
            place(&all_seeds, BoxSize::default()).unwrap(),
            "deterministic"
        );
    }

    #[test]
    fn promotion_and_relegation_override_utr() {
        let mut all_seeds = seeds(&[90, 85, 80, 75, 70, 65, 60, 55, 50, 45, 40, 35]);
        // A low-UTR player promoted from tier 2 moves up; a high one relegated moves down.
        all_seeds[11].previous = Some(Previous {
            tier: 2,
            movement: -1,
        });
        all_seeds[0].previous = Some(Previous {
            tier: 1,
            movement: 1,
        });
        // Relegated from the bottom tier stays in the bottom tier.
        all_seeds[10].previous = Some(Previous {
            tier: 2,
            movement: 1,
        });
        let boxes = place(&all_seeds, BoxSize::default()).unwrap();
        assert_eq!(boxes.len(), 2);
        assert!(boxes[0].entries.contains(&11), "{boxes:?}");
        assert!(boxes[1].entries.contains(&0), "{boxes:?}");
        assert!(boxes[1].entries.contains(&10));
        assert_eq!(boxes[0].entries.len(), 6);
        assert_eq!(boxes[1].entries.len(), 6);
    }

    #[test]
    fn every_entry_is_placed_exactly_once() {
        for n in 0..40 {
            let all_seeds = seeds(&(0..n).map(|idx| (idx * 37) % 100).collect::<Vec<_>>());
            let boxes = place(&all_seeds, BoxSize::default()).unwrap();
            let mut all: Vec<u32> = boxes
                .iter()
                .flat_map(|placed| placed.entries.clone())
                .collect();
            all.sort_unstable();
            assert_eq!(all, (0..u32::try_from(n).unwrap()).collect::<Vec<_>>());
            let tiers: Vec<u32> = boxes.iter().map(|placed| placed.tier).collect();
            assert_eq!(
                tiers,
                (1..=u32::try_from(boxes.len()).unwrap()).collect::<Vec<_>>()
            );
        }
    }

    fn seeds_of(n: usize) -> Vec<Seed<u32>> {
        let utrs: Vec<i64> = (0..n)
            .map(|idx| i64::try_from(idx * 37 % 100).unwrap())
            .collect();
        seeds(&utrs)
    }

    #[test]
    fn a_season_needs_two_entries_and_no_thin_box() {
        for n in 0..40 {
            let boxes = place(&seeds_of(n), BoxSize::default()).unwrap();
            let want = if n < 2 {
                Err(Unplayable::TooFewEntries)
            } else {
                Ok(())
            };
            assert_eq!(playable(n, &boxes), want, "n={n}");
        }
        // Small custom boxes still never strand a lone entry.
        for (min_size, max_size) in [(2, 2), (2, 3), (3, 3), (2, 16)] {
            let limits = BoxSize { min_size, max_size };
            for n in 2..40 {
                let boxes = place(&seeds_of(n), limits).unwrap();
                assert_eq!(
                    playable(n, &boxes),
                    Ok(()),
                    "n={n} in {min_size}..={max_size}"
                );
            }
        }
        let thin = [Placed {
            tier: 1,
            entries: vec![1_u32],
            utr_min: None,
            utr_max: None,
        }];
        assert_eq!(playable(3, &thin), Err(Unplayable::ThinBox));
        assert!(
            Unplayable::TooFewEntries
                .to_string()
                .contains("Fewer than two")
        );
    }

    #[test]
    fn promotion_and_relegation_by_position() {
        // Middle box of three: top two up, bottom two down.
        assert_eq!(movements(6, 2, 3, 2, 2), vec![-1, -1, 0, 0, 1, 1]);
        // Top box: nobody goes up.
        assert_eq!(movements(6, 1, 3, 2, 2), vec![0, 0, 0, 0, 1, 1]);
        // Bottom box: nobody goes down.
        assert_eq!(movements(7, 3, 3, 2, 2), vec![-1, -1, 0, 0, 0, 0, 0]);
        // A single box: everyone stays.
        assert_eq!(movements(8, 1, 1, 2, 2), vec![0; 8]);
        // Tiny boxes never move a player both ways.
        assert_eq!(movements(3, 2, 3, 2, 2), vec![-1, 0, 1]);
        assert_eq!(movements(1, 2, 3, 2, 2), vec![0]);
        assert!(movements(0, 2, 3, 2, 2).is_empty());
    }
}
