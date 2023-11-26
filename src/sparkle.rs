use rand::Rng;
use std::fmt::Write;

use crate::data::{Context, Error};

#[poise::command(slash_command, prefix_command)]
pub async fn shimmer(
    ctx: Context<'_>,
    #[description = "The dice you want to roll, like: `d4` or `3d6 1d10` or even just `6 8 10`"]
    dice: String,
) -> Result<(), Error> {
    let response = get_response(&dice);
    ctx.say(response).await?;
    Ok(())
}

fn get_response(dice: &String) -> String {
    let roll = DiceRollRequest::parse(&dice);
    let roll = match roll {
        Err(err) => {
            return err;
        }
        Ok(roll) => roll,
    };
    let mut roll = roll.roll();
    let resp = format!(
        "Rolling {}\n\nResult: {}",
        dice,
        roll.to_discord_markdown().trim()
    );
    if resp.len() > 1950 {
        format!(
            "Roll {}?? hoo.. that's a lot. I don't wanna flood the chat here, so, uh, I'll give you the quick summary:\n\n{}",
            dice,
            roll.short_summary()
        )
    } else {
        resp
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord)]
enum Die {
    D4,
    D6,
    D8,
    D10,
}

impl Die {
    fn sides(self) -> u64 {
        match self {
            Die::D4 => 4,
            Die::D6 => 6,
            Die::D8 => 8,
            Die::D10 => 10,
        }
    }
    fn bump_up(self) -> Die {
        match self {
            Die::D4 => Die::D6,
            Die::D6 => Die::D8,
            Die::D8 => Die::D10,
            Die::D10 => Die::D10,
        }
    }
    fn roll(self) -> Roll {
        let num = rand::thread_rng().gen_range(1..=self.sides());
        // A D10 is the biggest, it can't shimmer.
        if let Die::D10 = self {
            return Roll::Value(num, self);
        }
        if num == self.sides() {
            // shimmer!
            let bigger_die = self.bump_up();
            let bigger_roll = bigger_die.roll();
            match bigger_roll {
                Roll::Value(val, _) => Roll::Shimmer {
                    initial: self,
                    ultimate: bigger_die,
                    shimmer_count: 1,
                    value: num.max(val),
                },
                Roll::Shimmer {
                    ultimate,
                    value,
                    shimmer_count,
                    ..
                } => Roll::Shimmer {
                    initial: self,
                    ultimate,
                    shimmer_count: shimmer_count + 1,
                    value: num.max(value),
                },
            }
        } else {
            Roll::Value(num, self)
        }
    }
}
impl std::fmt::Display for Die {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_char('d')?;
        f.write_fmt(format_args!("{}", self.sides()))
    }
}

#[derive(Debug, Copy, Clone)]
enum Roll {
    Value(u64, Die),
    Shimmer {
        initial: Die,
        ultimate: Die,
        shimmer_count: u8,
        value: u64,
    },
}
impl Roll {
    fn is_shimmer(self) -> bool {
        match self {
            Roll::Shimmer { .. } => true,
            _ => false,
        }
    }
}

struct DiceRollRequest {
    dice: Vec<Die>,
}

impl DiceRollRequest {
    fn parse(s: &str) -> Result<Self, String> {
        let mut dice = Vec::new();
        for s in s.split_whitespace() {
            if s.trim().is_empty() {
                continue;
            }
            let (count, die) = DiceRollRequest::get_die_count(s)
                .ok_or_else(|| format!("Expected {} to be like XdY, e.g. 3d6 or 1d8", s))?;
            if count > 1_000_000 {
                return Err(format!(
                    "Hey buddy, I'm just a demigod, that's too many dice!",
                ));
            }
            for _ in 0..count {
                dice.push(die);
            }
        }
        Ok(DiceRollRequest { dice })
    }

    fn get_die_count(s: &str) -> Option<(u64, Die)> {
        if let Ok(sides) = s.trim().parse() {
            match sides {
                4 => return Some((1, Die::D4)),
                6 => return Some((1, Die::D6)),
                8 => return Some((1, Die::D8)),
                10 => return Some((1, Die::D10)),
                _ => return None,
            }
        }
        let idx = s.find('d')?;
        let (count, sides) = (&s[..idx], &s[idx + 1..]);
        let count: u64 = if count.trim().is_empty() {
            1
        } else {
            count.trim().parse().ok()?
        };
        let sides = sides.trim().parse().ok()?;
        match sides {
            4 => Some((count, Die::D4)),
            6 => Some((count, Die::D6)),
            8 => Some((count, Die::D8)),
            10 => Some((count, Die::D10)),
            _ => None,
        }
    }

    fn roll(self) -> RollResult {
        let mut rolls = Vec::new();
        for die in self.dice {
            rolls.push(die.roll());
        }
        RollResult { rolled_die: rolls }
    }
}

struct RollResult {
    rolled_die: Vec<Roll>,
}
impl RollResult {
    fn to_discord_markdown(&mut self) -> String {
        let mut s = String::new();
        for roll in self.rolled_die.iter() {
            match roll {
                Roll::Shimmer {
                    initial,
                    ultimate,
                    shimmer_count,
                    value,
                } => {
                    if *shimmer_count == 1 {
                        s.push_str(&format!(
                            "**{}** (d{} shimmered up to d{}) ",
                            value,
                            initial.sides(),
                            ultimate.sides()
                        ));
                    } else {
                        s.push_str(&format!(
                            "**{}** (d{} shimmered **{}** times up to d{}) ",
                            value,
                            initial.sides(),
                            shimmer_count,
                            ultimate.sides()
                        ));
                    }
                }
                Roll::Value(value, die) => {
                    s.push_str(&format!("{} (d{}) ", value, die.sides()));
                }
            }
        }
        s += "\n\n";
        s += &self.short_summary();
        s
    }

    fn short_summary(&mut self) -> String {
        let mut s = String::new();
        let shimmer_count = self.rolled_die.iter().filter(|r| r.is_shimmer()).count();
        if shimmer_count > 0 {
            s += &format!("{} Shimmers!\n", shimmer_count);
        }
        let highest_effect = self.get_highest_effect();
        let highest_total = self.get_highest_total();
        match (highest_effect, highest_total) {
            (
                FinalResult::Result {
                    total: etotal,
                    effect: eeffect,
                },
                FinalResult::Result {
                    total: ttotal,
                    effect: teffect,
                },
            ) => {
                if highest_effect == highest_total {
                    // There is one ideal interpretation
                    s += &format!("Total: {} (effect {})", etotal, eeffect);
                } else {
                    s.push_str(&format!("Best effect: {} (effect {})\n", etotal, eeffect));
                    s.push_str(&format!("Best total: {} (effect {})\n", ttotal, teffect));
                }
            }
        }
        s
    }

    fn get_highest_effect(&self) -> FinalResult {
        let vals = self
            .rolled_die
            .iter()
            .filter_map(|roll| match roll {
                Roll::Shimmer {
                    ultimate, value, ..
                } => Some((*value, *ultimate)),
                Roll::Value(value, die) => Some((*value, *die)),
            })
            .enumerate()
            .collect::<Vec<_>>();
        let effect = vals
            .iter()
            .max_by_key(|(_, (value, die))| (die.sides(), -((*value) as i128)));
        let (effect_idx, effect) = match effect {
            None => (0, Die::D4),
            Some((index, (_, die))) => (*index, *die),
        };
        if vals.len() < 3 {
            // too few rolls to have an effect die, fall back to a d4
            return FinalResult::Result {
                effect: Die::D4,
                total: vals.into_iter().map(|(_, (v, _))| v).sum(),
            };
        }
        let mut remaining_vals: Vec<_> = vals
            .into_iter()
            .filter(|(idx, _)| *idx != effect_idx)
            .map(|(_, (value, _))| value)
            .collect();
        remaining_vals.sort();
        let val = remaining_vals.iter().rev().take(2).sum();
        FinalResult::Result { total: val, effect }
    }

    fn get_highest_total(&mut self) -> FinalResult {
        self.rolled_die.sort_by_key(|roll| match roll {
            Roll::Shimmer {
                ultimate, value, ..
            } => (*value, -(ultimate.sides() as i128)),
            Roll::Value(v, d) => (*v, -(d.sides() as i128)),
        });
        let total = self
            .rolled_die
            .iter()
            .rev()
            .take(2)
            .filter_map(|roll| match roll {
                Roll::Value(v, _) => Some(*v),
                Roll::Shimmer { value, .. } => Some(*value),
            })
            .sum();
        let effect = self
            .rolled_die
            .iter()
            .rev()
            .skip(2)
            .filter_map(|d| match d {
                Roll::Shimmer { ultimate, .. } => Some(*ultimate),
                Roll::Value(_, die) => Some(*die),
            })
            .max()
            .unwrap_or(Die::D4);
        FinalResult::Result { total, effect }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FinalResult {
    Result { total: u64, effect: Die },
}

// Unit tests module
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_highest_effect() {
        let roll_result = RollResult {
            rolled_die: vec![
                Roll::Value(1, Die::D4),
                Roll::Value(2, Die::D6),
                Roll::Value(3, Die::D8),
            ],
        };

        assert_eq!(
            roll_result.get_highest_effect(),
            FinalResult::Result {
                total: 3,
                effect: Die::D8,
            }
        );
    }

    #[test]
    fn test_get_highest_total() {
        let mut roll_result = RollResult {
            rolled_die: vec![
                Roll::Value(1, Die::D4),
                Roll::Value(2, Die::D6),
                Roll::Value(3, Die::D8),
            ],
        };

        assert_eq!(
            roll_result.get_highest_total(),
            FinalResult::Result {
                total: 5,
                effect: Die::D4,
            }
        );

        let mut roll_result = RollResult {
            rolled_die: vec![
                Roll::Shimmer {
                    initial: Die::D4,
                    ultimate: Die::D8,
                    value: 6,
                    shimmer_count: 2,
                },
                Roll::Value(2, Die::D6),
                Roll::Value(3, Die::D8),
            ],
        };

        assert_eq!(
            roll_result.get_highest_total(),
            FinalResult::Result {
                total: 9,
                effect: Die::D6,
            }
        );
    }
}
