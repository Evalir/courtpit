//! Who may enter mixed doubles.
//!
//! Gender is collected only to validate mixed eligibility. A community picks the rule: the
//! classic one-female-one-male pairing, or any two players of different disclosed genders (so
//! `other` can play mixed). `undisclosed` is never eligible: the rule needs to know.

use serde::{Deserialize, Serialize};

/// A player's self-declared gender, as the eligibility rule sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Gender {
    /// Female.
    Female,
    /// Male.
    Male,
    /// Another gender.
    Other,
    /// Prefers not to say.
    Undisclosed,
}

/// The community's mixed-doubles eligibility rule (`settings.mixed_eligibility`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MixedEligibility {
    /// Exactly one female and one male player; `other` and `undisclosed` are ineligible.
    #[default]
    FemaleMale,
    /// Two players with different disclosed genders (female, male or other).
    AnyTwoDistinct,
}

/// Why a player or pair cannot enter mixed doubles. The `Display` text says why under the
/// active rule and is meant to be shown to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MixedError {
    /// A solo registrant's gender is not eligible under the rule.
    #[error("{}", solo_message(*rule))]
    Solo {
        /// The rule that refused them.
        rule: MixedEligibility,
    },
    /// The pair does not satisfy the rule.
    #[error("{}", pair_message(*rule))]
    Pair {
        /// The rule that refused them.
        rule: MixedEligibility,
    },
    /// Both players have the same gender under [`MixedEligibility::AnyTwoDistinct`].
    #[error(
        "mixed doubles needs two players of different genders (female, male or other); \
         these two players have the same gender"
    )]
    SameGender,
    /// A mixed entry has one or two players.
    #[error("mixed doubles entries have one or two players")]
    PlayerCount,
}

const fn solo_message(rule: MixedEligibility) -> &'static str {
    match rule {
        MixedEligibility::FemaleMale => {
            "mixed doubles needs one female and one male player; set your gender to female \
             or male on your profile to enter (other and undisclosed are not eligible)"
        }
        MixedEligibility::AnyTwoDistinct => {
            "mixed doubles needs two players of different genders; set your gender on your \
             profile to enter (undisclosed is not eligible)"
        }
    }
}

const fn pair_message(rule: MixedEligibility) -> &'static str {
    match rule {
        MixedEligibility::FemaleMale => {
            "mixed doubles needs one female and one male player (other and undisclosed are \
             not eligible)"
        }
        MixedEligibility::AnyTwoDistinct => {
            "mixed doubles needs two players of different genders, and both must have \
             disclosed theirs (undisclosed is not eligible)"
        }
    }
}

impl MixedEligibility {
    /// Whether a player with this gender can enter on their own (looking for a partner).
    #[must_use]
    pub const fn solo_eligible(self, gender: Gender) -> bool {
        match self {
            Self::FemaleMale => matches!(gender, Gender::Female | Gender::Male),
            Self::AnyTwoDistinct => !matches!(gender, Gender::Undisclosed),
        }
    }

    /// Checks the genders of an entry's players: one (a solo registrant, who must be
    /// individually eligible) or two (a pair, who must satisfy the rule together).
    pub fn check(self, genders: &[Gender]) -> Result<(), MixedError> {
        match *genders {
            [one] if self.solo_eligible(one) => Ok(()),
            [_] => Err(MixedError::Solo { rule: self }),
            [first, second] => match self {
                Self::FemaleMale => {
                    if matches!(
                        (first, second),
                        (Gender::Female, Gender::Male) | (Gender::Male, Gender::Female)
                    ) {
                        Ok(())
                    } else {
                        Err(MixedError::Pair { rule: self })
                    }
                }
                Self::AnyTwoDistinct => {
                    if !self.solo_eligible(first) || !self.solo_eligible(second) {
                        Err(MixedError::Pair { rule: self })
                    } else if first == second {
                        Err(MixedError::SameGender)
                    } else {
                        Ok(())
                    }
                }
            },
            _ => Err(MixedError::PlayerCount),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Gender::{Female, Male, Other, Undisclosed};
    use MixedEligibility::{AnyTwoDistinct, FemaleMale};

    const ALL: [Gender; 4] = [Female, Male, Other, Undisclosed];

    #[test]
    fn solo_registrants_table() {
        for (gender, female_male, any_two) in [
            (Female, true, true),
            (Male, true, true),
            (Other, false, true),
            (Undisclosed, false, false),
        ] {
            assert_eq!(FemaleMale.solo_eligible(gender), female_male, "{gender:?}");
            assert_eq!(AnyTwoDistinct.solo_eligible(gender), any_two, "{gender:?}");
            assert_eq!(FemaleMale.check(&[gender]).is_ok(), female_male);
            assert_eq!(AnyTwoDistinct.check(&[gender]).is_ok(), any_two);
        }
        assert_eq!(
            FemaleMale.check(&[Other]),
            Err(MixedError::Solo { rule: FemaleMale })
        );
    }

    #[test]
    fn every_pair_under_both_rules() {
        for first in ALL {
            for second in ALL {
                let female_male = matches!((first, second), (Female, Male) | (Male, Female));
                assert_eq!(
                    FemaleMale.check(&[first, second]).is_ok(),
                    female_male,
                    "{first:?} + {second:?} under female_male"
                );
                let any_two = first != second && first != Undisclosed && second != Undisclosed;
                assert_eq!(
                    AnyTwoDistinct.check(&[first, second]).is_ok(),
                    any_two,
                    "{first:?} + {second:?} under any_two_distinct"
                );
            }
        }
    }

    #[test]
    fn errors_say_why() {
        assert_eq!(
            FemaleMale.check(&[Female, Other]),
            Err(MixedError::Pair { rule: FemaleMale })
        );
        assert_eq!(
            AnyTwoDistinct.check(&[Other, Undisclosed]),
            Err(MixedError::Pair {
                rule: AnyTwoDistinct
            })
        );
        assert_eq!(
            AnyTwoDistinct.check(&[Other, Other]),
            Err(MixedError::SameGender)
        );
        assert_eq!(
            AnyTwoDistinct.check(&[Female, Female]),
            Err(MixedError::SameGender)
        );
        let text = |err: MixedError| err.to_string();
        assert!(text(MixedError::Pair { rule: FemaleMale }).contains("one female and one male"));
        assert!(
            text(MixedError::Solo { rule: FemaleMale }).contains("your profile"),
            "solo registrants are told what to change"
        );
        assert!(
            text(MixedError::Solo {
                rule: AnyTwoDistinct
            })
            .contains("undisclosed is not eligible")
        );
        assert!(text(MixedError::SameGender).contains("different genders"));
    }

    #[test]
    fn entries_have_one_or_two_players() {
        for rule in [FemaleMale, AnyTwoDistinct] {
            assert_eq!(rule.check(&[]), Err(MixedError::PlayerCount));
            assert_eq!(
                rule.check(&[Female, Male, Female]),
                Err(MixedError::PlayerCount)
            );
        }
    }

    #[test]
    fn rule_json_and_default() {
        assert_eq!(MixedEligibility::default(), FemaleMale);
        for (text, rule) in [
            ("\"female_male\"", FemaleMale),
            ("\"any_two_distinct\"", AnyTwoDistinct),
        ] {
            assert_eq!(
                serde_json::from_str::<MixedEligibility>(text).unwrap(),
                rule
            );
            assert_eq!(serde_json::to_string(&rule).unwrap(), text);
        }
        let _ = serde_json::from_str::<MixedEligibility>("\"anyone\"").unwrap_err();
        assert_eq!(
            serde_json::to_string(&Undisclosed).unwrap(),
            "\"undisclosed\""
        );
    }
}
