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

//! Alpha Strike scale helpers for the optional **1:1 "ground scale"** (hex) play: standard AS is
//! played in inches, but on a hex map every distance is halved and rounded up (2" = 1 hex), so
//! movement and the weapon-range brackets convert together. Pure + unit-tested.
//!
//! Range brackets (AS:CE): Short ≤6", Medium ≤24", Long ≤42", Extreme ≤60".

/// AS weapon range-bracket labels with their inch upper bounds (S/M/L/E).
const RANGE_BRACKETS_INCHES: [(&str, u32); 4] = [("S", 6), ("M", 24), ("L", 42), ("E", 60)];

/// Convert an inch distance to 1:1 ground (hex) scale: halved and rounded up (2" = 1 hex).
pub fn inches_to_hexes(inches: u32) -> u32 {
    inches.div_ceil(2)
}

/// Convert an Alpha Strike movement string to 1:1 ground (hex) scale. Each inch-denominated mode
/// (e.g. `8"`, `10"j`, `6"t`) is halved-rounded-up with the `"` dropped; modes joined by `/` keep
/// their separators, and non-inch modes (aerospace thrust like `7a`) pass through unchanged.
pub fn movement_hexes(mv: &str) -> String {
    mv.split('/')
        .map(|tok| {
            let tok = tok.trim();
            match tok.find('"') {
                Some(q) => {
                    let num: u32 = tok[..q].trim().parse().unwrap_or(0);
                    let suffix = &tok[q + 1..]; // the mode letter(s) after the quote, if any
                    format!("{}{}", inches_to_hexes(num), suffix)
                }
                None => tok.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// The weapon range brackets at 1:1 ground (hex) scale as a compact reference string:
/// `"S 0-3  M 4-12  L 13-21  E 22-30"`. Each bracket runs from the previous bracket's upper bound
/// +1 (Short from 0) to its own halved-rounded-up upper bound.
pub fn range_brackets_hexes() -> String {
    let mut prev = 0u32;
    let mut parts = Vec::with_capacity(RANGE_BRACKETS_INCHES.len());
    for (label, upper_in) in RANGE_BRACKETS_INCHES {
        let upper = inches_to_hexes(upper_in);
        let lower = if prev == 0 { 0 } else { prev + 1 };
        parts.push(format!("{label} {lower}-{upper}"));
        prev = upper;
    }
    parts.join("  ")
}

/// AS Phase-1 "self" to-hit target number for a range bracket index (0=S, 1=M, 2=L, 3=E).
/// Mirrors Mekbay's card math: skill + range offset (S 0 / M +2 / L +4 / E +6) + heat (linear,
/// the AS heat level *is* the modifier) + fire-control crits ×2 (+ crew crits ×2 for vehicles).
/// The opponent-dependent terms (target TMM, attacker movement) are §33 Phase 2 and excluded here.
pub fn as_to_hit(
    skill: u8,
    range_idx: usize,
    heat: u8,
    fire_control: u8,
    crew_hits: u8,
    is_vehicle: bool,
) -> u8 {
    const RANGE_OFFSET: [u8; 4] = [0, 2, 4, 6];
    let crew = if is_vehicle { crew_hits } else { 0 };
    skill + RANGE_OFFSET[range_idx] + heat + fire_control * 2 + crew * 2
}

/// AS attacker-movement to-hit modifier (§33 Phase 2, the "A" in SATOR). Unlike Classic (Walk +1 /
/// Run +2 / Jump +3), Alpha Strike only penalises **jumping**: +2 when the attacker jumped, else 0.
pub fn as_attacker_move_modifier(jumped: bool) -> i32 {
    if jumped {
        2
    } else {
        0
    }
}

/// AS target-movement to-hit modifier (§33 Phase 2, the "T" in SATOR): the target's TMM, +1 if it
/// jumped. An immobile target overrides both with a flat −4 (TMM ignored), per AS:CE.
pub fn as_target_modifier(tmm: u8, jumped: bool, immobile: bool) -> i32 {
    if immobile {
        -4
    } else {
        tmm as i32 + i32::from(jumped)
    }
}

/// Full AS to-hit target number for a range bracket: the Phase-1 "self" number ([`as_to_hit`]) plus
/// attacker movement and target movement (`target` = `(tmm, jumped, immobile)`, `None` when no
/// target is set). Floored at 2 — the minimum possible 2d6 target number.
#[allow(clippy::too_many_arguments)]
pub fn as_to_hit_full(
    skill: u8,
    range_idx: usize,
    heat: u8,
    fire_control: u8,
    crew_hits: u8,
    is_vehicle: bool,
    attacker_jumped: bool,
    target: Option<(u8, bool, bool)>,
) -> u8 {
    let base = as_to_hit(skill, range_idx, heat, fire_control, crew_hits, is_vehicle) as i32;
    let modifiers = as_attacker_move_modifier(attacker_jumped)
        + target.map_or(0, |(tmm, jumped, immobile)| as_target_modifier(tmm, jumped, immobile));
    (base + modifiers).max(2) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inches_halve_and_round_up() {
        assert_eq!(inches_to_hexes(8), 4);
        assert_eq!(inches_to_hexes(6), 3);
        assert_eq!(inches_to_hexes(5), 3); // 2.5 -> 3 (round up)
        assert_eq!(inches_to_hexes(0), 0);
    }

    #[test]
    fn movement_converts_each_mode_and_keeps_suffixes() {
        assert_eq!(movement_hexes("8\""), "4");
        assert_eq!(movement_hexes("6\"j"), "3j");
        assert_eq!(movement_hexes("10\"/8\"j"), "5/4j");
        assert_eq!(movement_hexes("5\"t"), "3t"); // tracked, rounds up
        assert_eq!(movement_hexes("7a"), "7a"); // aero thrust: no inches, unchanged
    }

    #[test]
    fn range_brackets_at_hex_scale() {
        assert_eq!(range_brackets_hexes(), "S 0-3  M 4-12  L 13-21  E 22-30");
    }

    #[test]
    fn to_hit_skill_plus_range_offset() {
        // skill 4, no heat/crits: S 4, M 6, L 8, E 10.
        assert_eq!(as_to_hit(4, 0, 0, 0, 0, false), 4);
        assert_eq!(as_to_hit(4, 1, 0, 0, 0, false), 6);
        assert_eq!(as_to_hit(4, 2, 0, 0, 0, false), 8);
        assert_eq!(as_to_hit(4, 3, 0, 0, 0, false), 10);
    }

    #[test]
    fn to_hit_heat_is_linear() {
        assert_eq!(as_to_hit(4, 0, 3, 0, 0, false), 7); // +3 heat
    }

    #[test]
    fn to_hit_fire_control_doubles() {
        assert_eq!(as_to_hit(4, 0, 0, 2, 0, false), 8); // 2 FC crits -> +4
    }

    #[test]
    fn to_hit_crew_only_for_vehicles() {
        assert_eq!(as_to_hit(4, 0, 0, 0, 1, true), 6); // vehicle: crew +2
        assert_eq!(as_to_hit(4, 0, 0, 0, 1, false), 4); // 'Mech: crew ignored
    }

    #[test]
    fn attacker_move_only_jump_penalised() {
        assert_eq!(as_attacker_move_modifier(false), 0);
        assert_eq!(as_attacker_move_modifier(true), 2);
    }

    #[test]
    fn target_modifier_tmm_jump_and_immobile() {
        assert_eq!(as_target_modifier(2, false, false), 2); // plain TMM
        assert_eq!(as_target_modifier(2, true, false), 3); // jumped +1
        assert_eq!(as_target_modifier(2, false, true), -4); // immobile overrides
    }

    #[test]
    fn to_hit_full_composes_self_attacker_target() {
        // skill 4, short, attacker jumped (+2), target TMM 2 jumped (+3): 4 + 0 + 2 + 3 = 9.
        assert_eq!(as_to_hit_full(4, 0, 0, 0, 0, false, true, Some((2, true, false))), 9);
        // No shot context → identical to the Phase-1 self number.
        assert_eq!(as_to_hit_full(4, 2, 0, 0, 0, false, false, None), as_to_hit(4, 2, 0, 0, 0, false));
    }

    #[test]
    fn to_hit_full_floors_at_two() {
        // skill 4, short, immobile target (−4) → 0, floored to the 2+ minimum.
        assert_eq!(as_to_hit_full(4, 0, 0, 0, 0, false, false, Some((0, false, true))), 2);
    }
}
