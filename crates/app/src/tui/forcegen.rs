// Neurohelmet — Copyright (C) 2026 Nate Palmer
//
// This file is part of Neurohelmet.
//
// Neurohelmet is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option) any later
// version.
//
// Neurohelmet is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR
// A PARTICULAR PURPOSE. See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// Neurohelmet. If not, see <https://www.gnu.org/licenses/>.

//! §35 Phase 1 — weighted random force generation. Pure logic (no UI): given the picker's hard
//! filters plus a faction/era/size/budget config, draw 'Mechs weighted by RATGenerator availability.
//!
//! Draw rules (decided 2026-06-27):
//!   - Candidates = 'Mechs passing the hard facets (type forced to 'Mech in Phase 1) whose
//!     availability score for the chosen faction/era is > 0. The "allow rare / off-table" knob
//!     relaxes that floor to include zero/no-data units — but **never future** ones (intro year
//!     after the era's end).
//!   - Weight = the availability score (`max(req,salvage)`), optionally skewed by a weight-class bias.
//!   - Termination: **budget is a hard ceiling, count is the target** (decided 2026-06-29, revising
//!     the original count-floor rule). Draw up to `count` units (capped by the roster's free slots),
//!     each restricted to what the remaining budget affords; stop early — short of `count` — once
//!     nothing else fits. With no budget, just draw `count`. To "spend a budget" with as many units
//!     as fit, set a large `count` (e.g. a Company) and let the budget cap it.
//!   - Duplicates are allowed (no per-chassis cap). The draw is seeded, so it reproduces.

use super::filters::{self, Filters};
use neurohelmet_core::data::bundle::Bundle;
use neurohelmet_core::domain::{GameMode, Mech, UnitType};

/// Named formations whose size matches a unit count (shown next to the size field).
pub const FORMATIONS: [(usize, &str); 4] =
    [(4, "Lance"), (5, "Star"), (6, "Level II"), (12, "Company")];

/// The optional weight-class bias values (skews the draw toward a class; never excludes).
pub const CLASS_BIAS: [&str; 4] = ["Light", "Medium", "Heavy", "Assault"];

/// Multiplier applied to a candidate's weight when it matches the chosen class bias.
const BIAS_FACTOR: u64 = 4;

/// The formation name for a unit count, if one matches exactly.
pub fn formation_name(count: usize) -> Option<&'static str> {
    FORMATIONS.iter().find(|(n, _)| *n == count).map(|(_, s)| *s)
}

/// Resolved generation parameters.
pub struct GenConfig<'a> {
    pub faction: Option<u16>,
    pub era_id: Option<u16>,
    /// The chosen era's end year, used to bar future units when `allow_rare` is on.
    pub era_to: Option<u16>,
    /// Target number of units (the formation size). The draw produces up to this many; a tight
    /// budget can leave it short.
    pub count: usize,
    /// Hard BV/PV ceiling. The total never exceeds it; the draw stops short of `count` if it must.
    pub budget: Option<u64>,
    pub allow_rare: bool,
    pub class_bias: Option<&'a str>,
    pub mode: GameMode,
    /// Free roster slots (`MAX_MECHS - current roster`). The result never exceeds this.
    pub max_units: usize,
}

/// The BV (Classic/Override) or PV (Alpha Strike) cost of a unit at default skills.
pub fn unit_cost(m: &Mech, mode: GameMode) -> u64 {
    match mode {
        GameMode::AlphaStrike | GameMode::StrategicBattleForce | GameMode::BattleForce | GameMode::AbstractCombatSystem => {
            u64::from(m.as_stats.pv)
        }
        GameMode::Classic | GameMode::Override => u64::from(m.bv),
    }
}

/// A candidate's draw weight for this config, or `None` if it isn't eligible.
fn candidate_weight(m: &Mech, cfg: &GenConfig) -> Option<u64> {
    let base = match m.avail_score(cfg.era_id, cfg.faction) {
        Some(s) => u64::from(s),
        // Off-table: only when the floor is relaxed, and never a future unit.
        None if cfg.allow_rare => {
            if cfg.era_to.is_some_and(|to| m.year > to) {
                return None;
            }
            1
        }
        None => return None,
    };
    let weight = match cfg.class_bias {
        Some(c) if m.weight_class == c => base * BIAS_FACTOR,
        _ => base,
    };
    Some(weight.max(1))
}

/// SplitMix64 — a tiny seeded PRNG so a generated force reproduces from its seed.
fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Pick an index into `pool` (`(bundle_index, weight)`) weighted by weight. `pool` must be nonempty.
fn weighted_pick(pool: &[(usize, u64)], state: &mut u64) -> usize {
    let total: u64 = pool.iter().map(|(_, w)| *w).sum();
    let mut r = next(state) % total;
    for &(idx, w) in pool {
        if r < w {
            return idx;
        }
        r -= w;
    }
    pool.last().map(|(i, _)| *i).unwrap()
}

/// How many units pass the hard filters + availability for this config (ignoring budget). Lets the
/// UI tell "no candidates at all" apart from "candidates exist but none fit the budget".
pub fn eligible_count(bundle: &Bundle, hard: &Filters, cfg: &GenConfig) -> usize {
    candidate_pool(bundle, hard, cfg).len()
}

/// Build the weighted candidate pool: 'Mechs passing the hard filters and eligible per `cfg`.
fn candidate_pool(bundle: &Bundle, hard: &Filters, cfg: &GenConfig) -> Vec<(usize, u64)> {
    bundle
        .mechs
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            if m.unit_type != UnitType::Mech {
                return None; // Phase 1 is 'Mech-only; mixed forces are Phase 2.
            }
            // AS-only hand-entered units can't join a Classic/Override session.
            if matches!(cfg.mode, GameMode::Classic | GameMode::Override) && m.is_as_only() {
                return None;
            }
            if !filters::matches(hard, m) {
                return None;
            }
            candidate_weight(m, cfg).map(|w| (i, w))
        })
        .collect()
}

/// Generate a force as a list of bundle indices (may repeat). Empty when no unit qualifies — or,
/// with a budget set, when even the cheapest candidate doesn't fit it.
///
/// Budget is a hard ceiling: the draw produces up to `count` units (capped by the free roster
/// slots), each restricted to what the remaining budget affords, and stops early — short of
/// `count` — once nothing else fits. With no budget it simply draws `count`.
pub fn generate(bundle: &Bundle, hard: &Filters, cfg: &GenConfig, seed: u64) -> Vec<usize> {
    let pool = candidate_pool(bundle, hard, cfg);
    if pool.is_empty() || cfg.max_units == 0 {
        return Vec::new();
    }
    let mut state = seed;
    let mut chosen = Vec::new();
    let mut spent = 0u64;
    let target = cfg.count.min(cfg.max_units);

    while chosen.len() < target {
        // With a budget, restrict each pick to what still fits; without one the whole pool is fair
        // game. As the budget depletes, expensive units drop out, so the draw self-balances.
        let affordable: Vec<(usize, u64)> = match cfg.budget {
            Some(budget) => {
                let remaining = budget.saturating_sub(spent);
                pool.iter()
                    .copied()
                    .filter(|(i, _)| unit_cost(&bundle.mechs[*i], cfg.mode) <= remaining)
                    .collect()
            }
            None => pool.clone(),
        };
        if affordable.is_empty() {
            break; // budget exhausted — stop short of `count` rather than blow past it
        }
        let i = weighted_pick(&affordable, &mut state);
        spent += unit_cost(&bundle.mechs[i], cfg.mode);
        chosen.push(i);
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use neurohelmet_core::data::bundle::Bundle;
    use neurohelmet_core::domain::{Location, LocationArmor};
    use std::collections::BTreeMap;

    fn mech(chassis: &str, class: &str, bv: u32, score: Option<u8>) -> Mech {
        let mut m = Mech {
            chassis: chassis.into(),
            tonnage: 50,
            bv,
            weight_class: class.into(),
            unit_type: UnitType::Mech,
            year: 3025,
            // A non-empty armor map keeps it from reading as an AS-only emplacement.
            armor: BTreeMap::from([(Location::CenterTorso, LocationArmor::default())]),
            ..Default::default()
        };
        if let Some(s) = score {
            // era 13, faction 27
            m.availability = BTreeMap::from([(13u16, BTreeMap::from([(27u16, s)]))]);
        }
        m
    }

    fn cfg(count: usize, budget: Option<u64>, allow_rare: bool, max_units: usize) -> GenConfig<'static> {
        GenConfig {
            faction: Some(27),
            era_id: Some(13),
            era_to: Some(3061),
            count,
            budget,
            allow_rare,
            class_bias: None,
            mode: GameMode::Classic,
            max_units,
        }
    }

    #[test]
    fn meets_count_floor_and_only_available_units() {
        let bundle = Bundle::new(vec![
            mech("Common", "Heavy", 1000, Some(80)),
            mech("Rare", "Heavy", 1000, Some(10)),
            mech("OffTable", "Heavy", 1000, None),
        ]);
        let force = generate(&bundle, &Filters::default(), &cfg(4, None, false, 12), 42);
        assert_eq!(force.len(), 4, "count is the floor");
        // OffTable (index 2) is not available to 27@13 and allow_rare is off -> never picked.
        assert!(force.iter().all(|&i| i != 2));
    }

    #[test]
    fn allow_rare_includes_offtable_but_not_future() {
        let mut future = mech("Future", "Heavy", 1000, None);
        future.chassis = "Future".into();
        future.year = 3100; // after era 13's end (3061)
        let bundle = Bundle::new(vec![mech("OffTable", "Heavy", 1000, None), future]);
        // Floor off: nothing qualifies.
        assert!(generate(&bundle, &Filters::default(), &cfg(2, None, false, 12), 1).is_empty());
        // Floor relaxed: OffTable qualifies, the future unit never does.
        let force = generate(&bundle, &Filters::default(), &cfg(4, None, true, 12), 1);
        assert!(!force.is_empty());
        assert!(force.iter().all(|&i| i == 0), "future unit must be barred");
    }

    #[test]
    fn budget_is_a_hard_ceiling_and_may_fall_short_of_count() {
        // Want 10 @ 1000 BV each under a 3500 budget -> only 3 fit (4th would hit 4000).
        let bundle = Bundle::new(vec![mech("A", "Heavy", 1000, Some(50))]);
        let force = generate(&bundle, &Filters::default(), &cfg(10, Some(3500), false, 12), 7);
        assert_eq!(force.len(), 3, "stops short of count to stay within budget");
        let total: u64 = force.iter().map(|&i| u64::from(bundle.mechs[i].bv)).sum();
        assert!(total <= 3500, "never exceeds the budget (got {total})");
    }

    #[test]
    fn count_caps_units_even_when_budget_is_generous() {
        // count 4 with a roomy budget -> exactly 4 (budget no longer pushes past count).
        let bundle = Bundle::new(vec![mech("A", "Heavy", 1000, Some(50))]);
        let force = generate(&bundle, &Filters::default(), &cfg(4, Some(100_000), false, 12), 7);
        assert_eq!(force.len(), 4);
    }

    #[test]
    fn no_unit_fits_the_budget_yields_empty() {
        // Cheapest candidate (1000) is over a 500 budget -> nothing fits.
        let bundle = Bundle::new(vec![mech("A", "Heavy", 1000, Some(50))]);
        let force = generate(&bundle, &Filters::default(), &cfg(4, Some(500), false, 12), 7);
        assert!(force.is_empty());
        // ...but the candidate pool itself is non-empty, so the UI can say "over budget" not
        // "no candidates".
        assert_eq!(eligible_count(&bundle, &Filters::default(), &cfg(4, Some(500), false, 12)), 1);
    }

    #[test]
    fn budget_balances_a_mixed_priced_lance() {
        // Cheap + expensive both available; a 3500 budget should prefer a fit that stays under it.
        let bundle = Bundle::new(vec![
            mech("Cheap", "Light", 750, Some(50)),
            mech("Dear", "Assault", 2800, Some(50)),
        ]);
        let force = generate(&bundle, &Filters::default(), &cfg(4, Some(3500), false, 12), 3);
        let total: u64 = force.iter().map(|&i| u64::from(bundle.mechs[i].bv)).sum();
        assert!(total <= 3500, "within budget (got {total})");
        assert!(!force.is_empty());
    }

    #[test]
    fn capped_by_free_roster_slots() {
        let bundle = Bundle::new(vec![mech("A", "Heavy", 100, Some(50))]);
        // count 6 but only 2 free slots.
        let force = generate(&bundle, &Filters::default(), &cfg(6, None, false, 2), 3);
        assert_eq!(force.len(), 2);
    }

    /// End-to-end against the real baked bundle (skipped if `data/mechs.bin` isn't present), proving
    /// faction/era resolution + the weighted draw line up on actual RATGenerator data.
    #[test]
    fn real_bundle_generates_an_available_lance() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/mechs.bin");
        let Ok(bundle) = Bundle::load(&path) else {
            return; // not baked in this environment — skip
        };
        let era = bundle.eras.iter().find(|e| e.name.contains("Early Succession")).unwrap();
        let fac = bundle.factions.iter().find(|f| f.name == "Federated Suns").unwrap();
        let cfg = GenConfig {
            faction: Some(fac.id),
            era_id: Some(era.id),
            era_to: Some(era.to),
            count: 4,
            budget: None,
            allow_rare: false,
            class_bias: None,
            mode: GameMode::Classic,
            max_units: 12,
        };
        let force = generate(&bundle, &Filters::default(), &cfg, 12345);
        assert_eq!(force.len(), 4, "a Davion lance");
        for &i in &force {
            let m = &bundle.mechs[i];
            assert_eq!(m.unit_type, UnitType::Mech);
            assert!(
                m.avail_score(Some(era.id), Some(fac.id)).is_some(),
                "{} must actually be available to {} in {}",
                m.display_name(),
                fac.name,
                era.name
            );
        }
    }

    #[test]
    fn reproducible_from_seed() {
        let bundle = Bundle::new(vec![
            mech("A", "Heavy", 1000, Some(50)),
            mech("B", "Heavy", 1000, Some(50)),
            mech("C", "Light", 1000, Some(50)),
        ]);
        let a = generate(&bundle, &Filters::default(), &cfg(5, None, false, 12), 999);
        let b = generate(&bundle, &Filters::default(), &cfg(5, None, false, 12), 999);
        assert_eq!(a, b);
    }
}
