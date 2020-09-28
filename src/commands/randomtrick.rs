use rand::seq::SliceRandom;

#[derive(Debug, Clone)]
pub struct Chance {
    what: String,
    chance_percent: usize,
    requires: Vec<Chance>,
}

impl Chance {
    pub fn of(what: &str, chance_percent: usize) -> Self {
        Chance {
            what: what.to_owned(),
            chance_percent,
            requires: vec![],
        }
    }

    pub fn with(&mut self, chances: &[Chance]) -> Self {
        if self.requires.is_empty() {
            self.requires = chances.to_vec();
        } else {
            for requirement in &mut self.requires {
                *requirement = requirement.with(chances);
            }
        }

        self.clone()
    }

    pub fn resolve(&self) -> String {
        if self.requires.is_empty() {
            return self.what.clone();
        } else {
            let req_resolved = from_chances(&self.requires);
            format!("{} {}", req_resolved, self.what).trim().replace("  ", " ")
        }
    }
}

lazy_static! {
    static ref STANCES: [Chance; 4] = [
        Chance::of("", 70), Chance::of("fakie", 40),
        Chance::of("nollie", 10), Chance::of("switch", 10)
    ];

    static ref SIDES: [Chance; 2] = [
        Chance::of("fs", 50), Chance::of("bs", 50),
    ];

    static ref MORE_SPEED: [Chance; 2] = [
        Chance::of("", 75), Chance::of("(на ходах)", 25),
    ];

    static ref TRICKS: Vec<Chance> = vec![
        // Slides & Grinds with regular stance
        Chance::of("boardslide", 15).with(&*SIDES),
        Chance::of("50-50", 15).with(&*SIDES),
        Chance::of("5-0", 10).with(&*SIDES),

        // Flips
        Chance::of("kickflip", 25).with(&*STANCES).with(&*MORE_SPEED),
        Chance::of("heelflip", 25).with(&*STANCES).with(&*MORE_SPEED),

        // Rotations
        Chance::of("180", 30).with(&*SIDES).with(&*MORE_SPEED),
        Chance::of("shove-it", 40).with(&*SIDES).with(&*STANCES).with(&*MORE_SPEED),

        // Misc
        Chance::of("no-comply 180", 10),
    ];
}

fn from_chances(chances: &[Chance]) -> String {
    let mut rng = rand::thread_rng();
    let result = chances.choose_weighted(&mut rng, |chance| chance.chance_percent).unwrap();

    result.resolve()
}

pub(crate) fn get() -> String {
    from_chances(&*TRICKS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    pub fn test_chances() {
        let mut frequencies = HashMap::<String, usize>::new();

        const MAX: usize = 1_000_000;
        for _ in 0..MAX {
            let trick = from_chances(&*TRICKS);

            *(frequencies.entry(trick.clone()).or_insert(0)) += 1;
        }

        let mut frequencies = frequencies.into_iter().collect::<Vec<_>>();
        frequencies.sort_by(|(_, hits_a), (_, hits_b)| hits_b.cmp(hits_a));

        for (trick, hits) in frequencies {
            println!("{}:  {}%", trick, hits as f32 / MAX as f32 * 100.0f32);
        }
    }
}