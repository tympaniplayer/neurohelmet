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

//! Skill-adjusted point cost: the Classic Battle Value skill multiplier (TechManual p.315) and
//! the Alpha Strike Point Value skill adjustment. Both are transcribed **verbatim** from MegaMek
//! (sourced, not recalled) so a force totals the same points a player would compute by the book:
//!
//! - BV table: `megamek/common/battlevalue/BVCalculator.java` (`bvMultipliers` / `bvSkillMultiplier`).
//! - AS PV:    `megamek/common/alphaStrike/conversion/ASPointValueConverter.java`
//!   (`getSkillAdjustedPointValue`).
//!
//! Skills run 0..=8, lower is better; the neutral baseline is Gunnery 4 / Piloting 5 for Classic
//! and a single Skill of 4 for Alpha Strike.

/// Battle Value skill multiplier, indexed `[gunnery][piloting]`, each clamped to 0..=8.
/// Verbatim from MegaMek `BVCalculator.bvMultipliers`; the neutral 4/5 cell is exactly 1.00.
const BV_MULTIPLIERS: [[f64; 9]; 9] = [
    [2.42, 2.31, 2.21, 2.10, 1.93, 1.75, 1.68, 1.59, 1.50],
    [2.21, 2.11, 2.02, 1.92, 1.76, 1.60, 1.54, 1.46, 1.38],
    [1.93, 1.85, 1.76, 1.68, 1.54, 1.40, 1.35, 1.28, 1.21],
    [1.66, 1.58, 1.51, 1.44, 1.32, 1.20, 1.16, 1.10, 1.04],
    [1.38, 1.32, 1.26, 1.20, 1.10, 1.00, 0.95, 0.90, 0.85],
    [1.31, 1.19, 1.13, 1.08, 0.99, 0.90, 0.86, 0.81, 0.77],
    [1.24, 1.12, 1.07, 1.02, 0.94, 0.85, 0.81, 0.77, 0.72],
    [1.17, 1.06, 1.01, 0.96, 0.88, 0.80, 0.76, 0.72, 0.68],
    [1.10, 0.99, 0.95, 0.90, 0.83, 0.75, 0.71, 0.68, 0.64],
];

/// The BV multiplier for a given gunnery/piloting (skills clamped to 0..=8, as MegaMek does).
pub fn bv_skill_multiplier(gunnery: u8, piloting: u8) -> f64 {
    let g = (gunnery as usize).min(8);
    let p = (piloting as usize).min(8);
    BV_MULTIPLIERS[g][p]
}

/// A unit's Battle Value adjusted for crew skill: `round(base_bv × multiplier)`
/// (MegaMek `BVCalculator`: `(int) Math.round(adjustedBV)`). A 0-BV spec (pre-bake data) stays 0.
pub fn skill_adjusted_bv(base_bv: u32, gunnery: u8, piloting: u8) -> u64 {
    if base_bv == 0 {
        return 0;
    }
    (base_bv as f64 * bv_skill_multiplier(gunnery, piloting)).round() as u64
}

/// A unit's Alpha Strike Point Value adjusted for its single Skill (0..=8, 4 = neutral).
/// Verbatim transcription of MegaMek `ASPointValueConverter.getSkillAdjustedPointValue`:
/// the per-point step grows with base PV (every +10 PV above 14 for worse skills, every +5 PV
/// above 7 for better skills), and the result floors at 1. A 0-PV spec (pre-bake data) stays 0.
pub fn skill_adjusted_pv(base_pv: u32, skill: u8) -> u64 {
    if base_pv == 0 || skill == 4 {
        return base_pv as u64;
    }
    let base = base_pv as i64;
    let skill = skill as i64;
    let mut multiplier: i64 = 1;
    let mut new_pv = base;
    if skill > 4 {
        if base > 14 {
            multiplier += (base - 5) / 10;
        }
        new_pv -= (skill - 4) * multiplier;
    } else {
        // skill < 4 (better than the baseline)
        if base > 7 {
            multiplier += (base - 3) / 5;
        }
        new_pv += (4 - skill) * multiplier;
    }
    new_pv.max(1) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_skills_leave_cost_unchanged() {
        // The 4/5 (Classic) and Skill-4 (AS) baselines must not move the cost.
        assert_eq!(skill_adjusted_bv(1897, 4, 5), 1897);
        assert!((bv_skill_multiplier(4, 5) - 1.0).abs() < 1e-9);
        assert_eq!(skill_adjusted_pv(52, 4), 52);
    }

    #[test]
    fn bv_scales_by_the_megamek_table() {
        // [gunnery][piloting] lookups against the verbatim table, rounded.
        // Elite 2/3 -> 1.68; round(1897 * 1.68) = round(3186.96) = 3187.
        assert_eq!(skill_adjusted_bv(1897, 2, 3), 3187);
        // Green 5/6 -> 0.86; round(1000 * 0.86) = 860.
        assert_eq!(skill_adjusted_bv(1000, 5, 6), 860);
        // Ace 0/0 -> 2.42; round(1000 * 2.42) = 2420.
        assert_eq!(skill_adjusted_bv(1000, 0, 0), 2420);
        // Out-of-range skills clamp to the 8/8 corner (0.64), like MegaMek.
        assert_eq!(skill_adjusted_bv(1000, 9, 9), 640);
    }

    #[test]
    fn pv_adjusts_by_the_megamek_formula() {
        // base 30, skill 3 (better): base>7 so step = 1 + (30-3)/5 = 6; 30 + (4-3)*6 = 36.
        assert_eq!(skill_adjusted_pv(30, 3), 36);
        // base 30, skill 5 (worse): base>14 so step = 1 + (30-5)/10 = 3; 30 - (5-4)*3 = 27.
        assert_eq!(skill_adjusted_pv(30, 5), 27);
        // base 52, skill 6: step = 1 + (52-5)/10 = 5; 52 - (6-4)*5 = 42.
        assert_eq!(skill_adjusted_pv(52, 6), 42);
        // Small unit, worse skill floors at 1 (never below).
        assert_eq!(skill_adjusted_pv(3, 8), 1);
        // Small unit, better skill, base<=7 keeps step 1: base 5, skill 2 -> 5 + (4-2)*1 = 7.
        assert_eq!(skill_adjusted_pv(5, 2), 7);
    }

    #[test]
    fn zero_cost_specs_stay_zero() {
        assert_eq!(skill_adjusted_bv(0, 0, 0), 0);
        assert_eq!(skill_adjusted_pv(0, 0), 0);
    }
}
