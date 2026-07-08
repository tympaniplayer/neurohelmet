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

//! GATOR — the Classic BattleTech weapon-attack to-hit assembly (§24). The community mnemonic:
//! **G**unnery + **A**ttacker movement + **T**arget movement + **O**ther + **R**ange. Mirrors
//! Mekbay's `inventory-target-number.util.ts`. Pure + unit-tested; the dice roll stays manual — we
//! only assemble the target number, consistent with the crit/PSR philosophy.
//!
//! The opponent-dependent half (the target's range + movement) needs a *target*, which the player
//! hand-enters (distance in hexes + how the target moved); see `session::CtTarget`. The attacker
//! half — gunnery, own movement, equipment, heat — is already tracked on the unit.

use super::movement::{target_movement_modifier, MoveMode};

/// A weapon-attack range bracket, selected from the distance to the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RangeBracket {
    Short,
    Medium,
    Long,
    Extreme,
}

impl RangeBracket {
    /// The to-hit modifier for this bracket (TW: short +0, medium +2, long +4, extreme +6).
    pub fn modifier(self) -> i32 {
        match self {
            RangeBracket::Short => 0,
            RangeBracket::Medium => 2,
            RangeBracket::Long => 4,
            RangeBracket::Extreme => 6,
        }
    }

    /// Single-letter label for the compact weapon-row display.
    pub fn code(self) -> &'static str {
        match self {
            RangeBracket::Short => "S",
            RangeBracket::Medium => "M",
            RangeBracket::Long => "L",
            RangeBracket::Extreme => "E",
        }
    }
}

/// Parse a printed weapon range string (`"7/14/21"`, also a 4-value `"3/6/9/12"`) into the
/// short/medium/long maximum-hex thresholds. Returns `None` for strings we can't bracket — melee /
/// physical (empty), single-value infantry ranges (`"2"`), or anything non-numeric.
pub fn parse_ranges(range: &str) -> Option<(u16, u16, u16)> {
    let mut it = range.split('/').map(|t| t.trim().parse::<u16>().ok());
    match (it.next(), it.next(), it.next()) {
        (Some(Some(s)), Some(Some(m)), Some(Some(l))) => Some((s, m, l)),
        _ => None,
    }
}

/// Select the range bracket for `distance` hexes against a weapon whose short/medium/long maximum
/// thresholds are `(short, med, long)`. Extreme is the TW optional bracket (`long < d ≤ 2 × long`).
/// `None` = out of range (beyond extreme) — the weapon cannot reach.
pub fn range_bracket(short: u16, med: u16, long: u16, distance: u16) -> Option<RangeBracket> {
    if distance <= short {
        Some(RangeBracket::Short)
    } else if distance <= med {
        Some(RangeBracket::Medium)
    } else if distance <= long {
        Some(RangeBracket::Long)
    } else if distance <= long.saturating_mul(2) {
        Some(RangeBracket::Extreme)
    } else {
        None
    }
}

/// Minimum-range penalty (TW): `(min − distance) + 1` when `0 < distance ≤ min`, else 0. neurohelmet
/// doesn't yet bake per-weapon minimum range — it isn't in the `s/m/l` range cell the bundle is
/// built from (Mekbay reads it off the unit SVG) — so callers pass `min = 0` until that data lands;
/// the helper is here for when it does.
pub fn minimum_range_modifier(min: u16, distance: u16) -> i32 {
    if min == 0 || distance == 0 || distance > min {
        0
    } else {
        (min as i32 - distance as i32) + 1
    }
}

/// The target-side to-hit modifier (the "T" in GATOR): the target's Target Movement Modifier from
/// how many hexes it moved (+1 if it jumped, via [`target_movement_modifier`]), or a flat −4 when
/// the target is immobile (overrides movement, per TW).
pub fn target_modifier(hexes_moved: u8, jumped: bool, immobile: bool) -> i32 {
    if immobile {
        -4
    } else {
        let mode = if jumped {
            MoveMode::Jumped
        } else {
            MoveMode::Walked
        };
        target_movement_modifier(hexes_moved, mode)
    }
}

/// Assemble the full Classic to-hit target number for one weapon, floored at the 2+ minimum (a 2d6
/// roll can never beat 2). All terms are pre-resolved by the caller:
/// - `gunnery` — the pilot's gunnery skill (the base "G").
/// - `attacker_move` — the attacker's own movement modifier (A; walked +1 / ran +2 / jumped +3).
/// - `target` — the target-side modifier (T; from [`target_modifier`]).
/// - `range` — the range-bracket modifier (R; from [`RangeBracket::modifier`]).
/// - `min_range` — the minimum-range penalty (from [`minimum_range_modifier`]).
/// - `other` — equipment-derived (TC / pulse, the §12 `weapon_to_hit`) plus the heat fire penalty (O).
pub fn to_hit(
    gunnery: u8,
    attacker_move: i32,
    target: i32,
    range: i32,
    min_range: i32,
    other: i32,
) -> i32 {
    (gunnery as i32 + attacker_move + target + range + min_range + other).max(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_three_and_four_value_ranges() {
        assert_eq!(parse_ranges("7/14/21"), Some((7, 14, 21)));
        assert_eq!(parse_ranges("3/6/9/12"), Some((3, 6, 9))); // extra value ignored
        assert_eq!(parse_ranges("2"), None); // infantry single value
        assert_eq!(parse_ranges(""), None); // melee/physical
        assert_eq!(parse_ranges("1/2/x"), None); // non-numeric
    }

    #[test]
    fn bracket_selection_incl_extreme_and_out_of_range() {
        let (s, m, l) = (7, 14, 21);
        assert_eq!(range_bracket(s, m, l, 1), Some(RangeBracket::Short));
        assert_eq!(range_bracket(s, m, l, 7), Some(RangeBracket::Short));
        assert_eq!(range_bracket(s, m, l, 8), Some(RangeBracket::Medium));
        assert_eq!(range_bracket(s, m, l, 14), Some(RangeBracket::Medium));
        assert_eq!(range_bracket(s, m, l, 21), Some(RangeBracket::Long));
        assert_eq!(range_bracket(s, m, l, 22), Some(RangeBracket::Extreme));
        assert_eq!(range_bracket(s, m, l, 42), Some(RangeBracket::Extreme)); // 2× long
        assert_eq!(range_bracket(s, m, l, 43), None); // beyond extreme
    }

    #[test]
    fn bracket_modifiers() {
        assert_eq!(RangeBracket::Short.modifier(), 0);
        assert_eq!(RangeBracket::Medium.modifier(), 2);
        assert_eq!(RangeBracket::Long.modifier(), 4);
        assert_eq!(RangeBracket::Extreme.modifier(), 6);
    }

    #[test]
    fn minimum_range_penalty() {
        // LRM min range 6: at 3 hexes → (6 − 3) + 1 = +4; at 6 → +1; beyond min → 0.
        assert_eq!(minimum_range_modifier(6, 3), 4);
        assert_eq!(minimum_range_modifier(6, 6), 1);
        assert_eq!(minimum_range_modifier(6, 7), 0);
        assert_eq!(minimum_range_modifier(0, 3), 0); // no minimum range
    }

    #[test]
    fn target_modifier_from_hexes_jump_immobile() {
        assert_eq!(target_modifier(0, false, false), 0); // stood still
        assert_eq!(target_modifier(5, false, false), 2); // 5 hexes → TMM 2
        assert_eq!(target_modifier(5, true, false), 3); // jumped → +1
        assert_eq!(target_modifier(5, false, true), -4); // immobile overrides
    }

    #[test]
    fn to_hit_sums_all_terms() {
        // gunnery 4, ran (+2), target TMM 2, medium range (+2), no min, TC (−1): 4+2+2+2+0−1 = 9.
        assert_eq!(to_hit(4, 2, 2, 2, 0, -1), 9);
    }

    #[test]
    fn to_hit_floors_at_two() {
        // gunnery 3, stationary, immobile target (−4), short range: 3 − 4 = −1 → floored to 2.
        assert_eq!(to_hit(3, 0, -4, 0, 0, 0), 2);
    }
}
