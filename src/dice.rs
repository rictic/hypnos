use rand::Rng;
use std::fmt::Write;

use crate::data::{Context, Error};

#[poise::command(slash_command, prefix_command)]
pub async fn roll(
    ctx: Context<'_>,
    #[description = "The dice you want to roll, like: `d4` or `3d6 1d10` or even just `6 8 10`"]
    dice: String,
) -> Result<(), Error> {
    let response = get_response(&dice);
    ctx.say(format!("Zim: {}", response)).await?;
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
struct Die {
    sides: u64,
}

impl Die {
    fn roll(self) -> Roll {
        let num = rand::thread_rng().gen_range(1..=self.sides);
        if num == 1 {
            Roll::Glitch(self)
        } else {
            Roll::Value(num, self)
        }
    }
}
impl std::fmt::Display for Die {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_char('d')?;
        f.write_fmt(format_args!("{}", self.sides))
    }
}

#[derive(Debug, Clone, Copy)]
enum Roll {
    Glitch(Die),
    Value(u64, Die),
}
impl Roll {
    fn is_glitch(self) -> bool {
        match self {
            Roll::Glitch(_) => true,
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
            return Some((1, Die { sides }));
        }
        let idx = s.find('d')?;
        let (count, sides) = (&s[..idx], &s[idx + 1..]);
        let count: u64 = if count.trim().is_empty() {
            1
        } else {
            count.trim().parse().ok()?
        };
        let sides = sides.trim().parse().ok()?;
        Some((count, Die { sides }))
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
    fn is_botch(&self) -> bool {
        self.rolled_die.iter().all(|r| r.is_glitch())
    }

    fn to_discord_markdown(&mut self) -> String {
        let mut s = String::new();
        for roll in self.rolled_die.iter() {
            match roll {
                Roll::Glitch(die) => {
                    s.push_str(&format!("**1** (d{}) ", die.sides));
                }
                Roll::Value(value, die) => {
                    s.push_str(&format!("{} (d{}) ", value, die.sides));
                }
            }
        }
        s += "\n\n";
        s += &self.short_summary();
        s
    }

    fn short_summary(&mut self) -> String {
        let mut s = String::new();
        if self.is_botch() {
            s += "**BOTCH!**";
            return s;
        }
        let glitch_count = self.rolled_die.iter().filter(|r| r.is_glitch()).count();
        if glitch_count > 0 {
            s += &format!("{} Glitches!\n", glitch_count);
        }
        let highest_effect = self.get_highest_effect();
        let highest_total = self.get_highest_total();
        match (highest_effect, highest_total) {
            (CortexResult::Botch, _) => {
                return "Internal error, disagreement on botch??".to_string();
            }
            (_, CortexResult::Botch) => {
                return "Internal error, disagreement on botch??".to_string();
            }
            (
                CortexResult::Result {
                    total: etotal,
                    effect: eeffect,
                },
                CortexResult::Result {
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

    fn get_highest_effect(&self) -> CortexResult {
        let non_glitches = self
            .rolled_die
            .iter()
            .filter_map(|roll| match roll {
                Roll::Glitch(_) => None,
                Roll::Value(value, die) => Some((*value, *die)),
            })
            .enumerate()
            .collect::<Vec<_>>();
        let effect = non_glitches
            .iter()
            .max_by_key(|(_, (value, die))| (die.sides, -((*value) as i128)));
        let (effect_idx, effect) = match effect {
            None => return CortexResult::Botch,
            Some((index, (_, die))) => (*index, *die),
        };
        if non_glitches.len() < 3 {
            // too few rolls to have an effect die, fall back to a d4
            return CortexResult::Result {
                effect: Die { sides: 4 },
                total: non_glitches.into_iter().map(|(_, (v, _))| v).sum(),
            };
        }
        let mut remaining_vals: Vec<_> = non_glitches
            .into_iter()
            .filter(|(idx, _)| *idx != effect_idx)
            .map(|(_, (value, _))| value)
            .collect();
        remaining_vals.sort();
        let val = remaining_vals.iter().rev().take(2).sum();
        CortexResult::Result { total: val, effect }
    }

    fn get_highest_total(&mut self) -> CortexResult {
        self.rolled_die.sort_by_key(|roll| match roll {
            Roll::Glitch(_) => (0, 0),
            Roll::Value(v, d) => (*v, -(d.sides as i128)),
        });
        let total = self
            .rolled_die
            .iter()
            .rev()
            .take(2)
            .filter_map(|roll| {
                if let Roll::Value(v, _) = roll {
                    Some(*v)
                } else {
                    None
                }
            })
            .sum();
        if total == 0 {
            return CortexResult::Botch;
        }
        let effect = self
            .rolled_die
            .iter()
            .rev()
            .skip(2)
            .filter_map(|d| {
                if let Roll::Value(_, d) = d {
                    Some(*d)
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(Die { sides: 4 });
        CortexResult::Result { total, effect }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CortexResult {
    Botch,
    Result { total: u64, effect: Die },
}
