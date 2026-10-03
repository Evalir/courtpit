//! Schedules: round-robin rounds (circle method) and single-elimination draws.
//!
//! Both are generic over the participant type so they can be tested with plain integers and
//! used with [`crate::ids::EntryId`] by the server. Output is fully determined by the input
//! order.

/// One pairing: `a` plays on side A, `b` on side B.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::min_ident_chars,
    reason = "fields `a` and `b` are the established names for the two sides"
)]
pub struct Pairing<T> {
    /// Participant on side A.
    pub a: T,
    /// Participant on side B.
    pub b: T,
}

/// A full single round robin: every participant meets every other exactly once.
///
/// Circle method: participant 0 stays fixed while the others rotate. With an odd count a
/// phantom "bye" joins the circle; whoever meets it sits the round out (each participant
/// exactly once). Rounds are `n - 1` for even `n` and `n` for odd `n`. Sides alternate so
/// that over the season everyone is side A about half the time.
#[must_use]
pub fn round_robin<T: Copy>(participants: &[T]) -> Vec<Vec<Pairing<T>>> {
    let mut slots: Vec<Option<T>> = participants.iter().copied().map(Some).collect();
    if slots.len() < 2 {
        return Vec::new();
    }
    if slots.len() % 2 == 1 {
        slots.push(None);
    }
    let n = slots.len();
    let mut rounds = Vec::with_capacity(n - 1);
    for round in 0..n - 1 {
        let mut pairs = Vec::with_capacity(n / 2);
        for i in 0..n / 2 {
            let (x, y) = (slots[i], slots[n - 1 - i]);
            if let (Some(x), Some(y)) = (x, y) {
                // Alternate the fixed participant's side by round, the others by position.
                let flip = if i == 0 { round % 2 == 1 } else { i % 2 == 1 };
                pairs.push(if flip {
                    Pairing { a: y, b: x }
                } else {
                    Pairing { a: x, b: y }
                });
            }
        }
        rounds.push(pairs);
        // Rotate everyone but slot 0 one step clockwise.
        slots[1..].rotate_right(1);
    }
    rounds
}

/// Seed positions in a bracket of `size` (a power of two), top to bottom: for 8 it is
/// `[1, 8, 4, 5, 2, 7, 3, 6]`, so seeds 1 and 2 can only meet in the final and the top
/// seeds face the lowest ones first.
#[must_use]
pub fn bracket_positions(size: usize) -> Vec<usize> {
    let mut seeds = vec![1];
    while seeds.len() < size {
        let total = seeds.len() * 2 + 1;
        seeds = seeds
            .iter()
            .flat_map(|&seed| [seed, total - seed])
            .collect();
    }
    seeds
}

/// First-round matches of a single-elimination draw for participants in seed order (best
/// first). The draw is padded to the next power of two with byes (`None`), which go to the
/// top seeds. A pairing with a `None` side is a bye: the other side advances.
#[must_use]
pub fn single_elimination<T: Copy>(seeded: &[T]) -> Vec<(Option<T>, Option<T>)> {
    if seeded.len() < 2 {
        return Vec::new();
    }
    let size = seeded.len().next_power_of_two();
    let at = |seed: usize| seeded.get(seed - 1).copied();
    bracket_positions(size)
        .chunks(2)
        .map(|pair| (at(pair[0]), at(pair[1])))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    fn check_round_robin(n: u32) {
        let people: Vec<u32> = (0..n).collect();
        let rounds = round_robin(&people);
        let expected_rounds = if n < 2 {
            0
        } else if n.is_multiple_of(2) {
            n - 1
        } else {
            n
        };
        assert_eq!(rounds.len(), expected_rounds as usize, "n={n}");

        let mut seen = HashSet::new();
        let mut byes: HashMap<u32, u32> = HashMap::new();
        let mut side_a: HashMap<u32, u32> = HashMap::new();
        for round in &rounds {
            let mut playing = HashSet::new();
            for pairing in round {
                assert_ne!(pairing.a, pairing.b);
                assert!(
                    playing.insert(pairing.a) && playing.insert(pairing.b),
                    "twice in a round"
                );
                let key = (pairing.a.min(pairing.b), pairing.a.max(pairing.b));
                assert!(seen.insert(key), "pair {key:?} repeated (n={n})");
                *side_a.entry(pairing.a).or_default() += 1;
            }
            for x in &people {
                if !playing.contains(x) {
                    *byes.entry(*x).or_default() += 1;
                }
            }
        }
        assert_eq!(
            seen.len() as u32,
            n * n.saturating_sub(1) / 2,
            "every pair once"
        );
        if n % 2 == 1 && n > 1 {
            assert!(
                people.iter().all(|x| byes.get(x) == Some(&1)),
                "one bye each"
            );
        } else {
            assert!(byes.is_empty());
        }
        for x in &people {
            let games = n - 1;
            let wins_a = side_a.get(x).copied().unwrap_or(0);
            assert!(
                wins_a.abs_diff(games - wins_a) <= 2 || n < 2,
                "n={n} x={x} side A {wins_a}/{games}"
            );
        }
    }

    #[test]
    fn round_robin_every_size_up_to_twelve() {
        for n in 0..=12 {
            check_round_robin(n);
        }
    }

    #[test]
    fn round_robin_is_deterministic() {
        assert_eq!(round_robin(&[1, 2, 3, 4, 5]), round_robin(&[1, 2, 3, 4, 5]));
        let four = round_robin(&['a', 'b', 'c', 'd']);
        assert_eq!(
            four[0],
            vec![Pairing { a: 'a', b: 'd' }, Pairing { a: 'c', b: 'b' }]
        );
    }

    #[test]
    fn bracket_positions_standard() {
        assert_eq!(bracket_positions(1), vec![1]);
        assert_eq!(bracket_positions(2), vec![1, 2]);
        assert_eq!(bracket_positions(4), vec![1, 4, 2, 3]);
        assert_eq!(bracket_positions(8), vec![1, 8, 4, 5, 2, 7, 3, 6]);
        let sixteen = bracket_positions(16);
        assert_eq!(sixteen.len(), 16);
        assert_eq!(sixteen.iter().collect::<HashSet<_>>().len(), 16);
        // Seeds 1 and 2 in different halves, 1–4 in different quarters.
        let pos = |seed| sixteen.iter().position(|&x| x == seed).unwrap();
        assert!(pos(1) < 8 && pos(2) >= 8);
        let quarters: HashSet<usize> = (1..=4).map(|seed| pos(seed) / 4).collect();
        assert_eq!(quarters.len(), 4);
    }

    #[test]
    fn single_elimination_pads_with_byes_for_top_seeds() {
        let draw = single_elimination(&[10, 20, 30, 40, 50]);
        assert_eq!(draw.len(), 4, "padded to 8");
        assert_eq!(
            draw,
            vec![
                (Some(10), None),
                (Some(40), Some(50)),
                (Some(20), None),
                (Some(30), None),
            ]
        );
        let byes = draw
            .iter()
            .filter(|(side_a, side_b)| side_a.is_none() || side_b.is_none())
            .count();
        assert_eq!(byes, 3);
        assert!(single_elimination::<u8>(&[1]).is_empty());
        assert_eq!(single_elimination(&[1, 2]), vec![(Some(1), Some(2))]);
        let full = single_elimination(&(1..=8).collect::<Vec<_>>());
        assert!(
            full.iter()
                .all(|(side_a, side_b)| side_a.is_some() && side_b.is_some())
        );
    }
}
