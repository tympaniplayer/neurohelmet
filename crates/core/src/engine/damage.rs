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

//! Per-location damage tracking. Mirrors Mekbay's model: the maxima live in the immutable
//! [`LocationArmor`], damage is a separate integer hit counter, and remaining = max - hits.

use crate::domain::{Facing, LocationArmor};
use serde::{Deserialize, Serialize};

/// Mutable damage state for one location. Front and rear armor are separate pools that share
/// a single internal-structure pool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocState {
    pub armor_hits: u16,
    pub rear_hits: u16,
    pub internal_hits: u16,
}

/// What happened when damage was applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageOutcome {
    /// All damage stopped at armor.
    Absorbed,
    /// `n` points carried through to internal structure (location still standing).
    IntoInternal(u16),
    /// Internal structure reduced to zero — location destroyed (no leftover damage).
    Destroyed,
    /// Location destroyed and `n` points of damage had nowhere to go (caller may transfer).
    Excess(u16),
}

/// The location that absorbs overflow when `loc` is destroyed (standard biped transfer
/// diagram): arms/legs → same-side torso, side torso → center torso. `None` means the mech is
/// dead (head or center torso) and excess is lost.
pub fn transfer_to(loc: crate::domain::Location) -> Option<crate::domain::Location> {
    use crate::domain::Location::*;
    match loc {
        LeftArm | LeftLeg | FrontLeftLeg | RearLeftLeg => Some(LeftTorso),
        RightArm | RightLeg | FrontRightLeg | RearRightLeg => Some(RightTorso),
        LeftTorso | RightTorso | CenterLeg => Some(CenterTorso),
        // Aerospace: every armor arc spills into the one shared Structural Integrity pool; SI is
        // terminal (when it's gone the fighter is destroyed, excess lost).
        Nose | LeftWing | RightWing | Aft => Some(AeroSI),
        // Head/CT/AeroSI are terminal; vehicle locations don't cascade (their path is separate).
        _ => None,
    }
}

/// Saturating remaining points.
pub fn remaining(max: u16, hits: u16) -> u16 {
    max.saturating_sub(hits)
}

/// Armor remaining for the given facing.
pub fn armor_remaining(max: &LocationArmor, st: &LocState, facing: Facing) -> u16 {
    match facing {
        Facing::Front => remaining(max.armor_max, st.armor_hits),
        Facing::Rear => remaining(max.rear_max, st.rear_hits),
    }
}

/// Internal structure remaining.
pub fn internal_remaining(max: &LocationArmor, st: &LocState) -> u16 {
    remaining(max.internal_max, st.internal_hits)
}

/// True once internal structure is gone.
pub fn is_destroyed(max: &LocationArmor, st: &LocState) -> bool {
    st.internal_hits >= max.internal_max && max.internal_max > 0
}

/// Apply `amount` points of damage to `facing`. Armor absorbs first; overflow goes to the
/// shared internal structure; overflow past internal is returned as [`DamageOutcome::Excess`].
pub fn apply_damage(
    max: &LocationArmor,
    st: &mut LocState,
    facing: Facing,
    amount: u16,
) -> DamageOutcome {
    let armor_avail = armor_remaining(max, st, facing);
    let to_armor = amount.min(armor_avail);
    match facing {
        Facing::Front => st.armor_hits += to_armor,
        Facing::Rear => st.rear_hits += to_armor,
    }
    let mut leftover = amount - to_armor;
    if leftover == 0 {
        return DamageOutcome::Absorbed;
    }

    let internal_avail = internal_remaining(max, st);
    let to_internal = leftover.min(internal_avail);
    st.internal_hits += to_internal;
    leftover -= to_internal;

    // Anything past both armor and internal overflows (→ caller cascades it). For a location with
    // no internal of its own (an aerospace arc, internal_max 0) this is how its armor-overflow
    // reaches the shared SI pool. For internal-bearing locations leftover>0 ⟺ internal just filled,
    // so this still means "destroyed, with excess".
    if leftover > 0 {
        DamageOutcome::Excess(leftover)
    } else if is_destroyed(max, st) {
        DamageOutcome::Destroyed
    } else {
        DamageOutcome::IntoInternal(to_internal)
    }
}

/// Repair (remove) up to `amount` hits from `facing`'s armor.
pub fn repair_armor(st: &mut LocState, facing: Facing, amount: u16) {
    match facing {
        Facing::Front => st.armor_hits = st.armor_hits.saturating_sub(amount),
        Facing::Rear => st.rear_hits = st.rear_hits.saturating_sub(amount),
    }
}

/// Repair (remove) up to `amount` internal-structure hits.
pub fn repair_internal(st: &mut LocState, amount: u16) {
    st.internal_hits = st.internal_hits.saturating_sub(amount);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn max(a: u16, r: u16, i: u16) -> LocationArmor {
        LocationArmor {
            armor_max: a,
            rear_max: r,
            internal_max: i,
        }
    }

    #[test]
    fn absorbed_by_armor() {
        let m = max(10, 0, 5);
        let mut st = LocState::default();
        assert_eq!(
            apply_damage(&m, &mut st, Facing::Front, 4),
            DamageOutcome::Absorbed
        );
        assert_eq!(st.armor_hits, 4);
        assert_eq!(armor_remaining(&m, &st, Facing::Front), 6);
    }

    #[test]
    fn overflow_into_internal() {
        let m = max(3, 0, 5);
        let mut st = LocState::default();
        // 3 to armor, 2 to internal
        assert_eq!(
            apply_damage(&m, &mut st, Facing::Front, 5),
            DamageOutcome::IntoInternal(2)
        );
        assert_eq!(st.armor_hits, 3);
        assert_eq!(st.internal_hits, 2);
        assert!(!is_destroyed(&m, &st));
    }

    #[test]
    fn destroyed_with_excess() {
        let m = max(3, 0, 5);
        let mut st = LocState::default();
        // 3 to armor, 5 to internal (destroyed), 2 excess
        assert_eq!(
            apply_damage(&m, &mut st, Facing::Front, 10),
            DamageOutcome::Excess(2)
        );
        assert!(is_destroyed(&m, &st));
        assert_eq!(internal_remaining(&m, &st), 0);
    }

    #[test]
    fn exact_destruction_no_excess() {
        let m = max(2, 0, 3);
        let mut st = LocState::default();
        assert_eq!(
            apply_damage(&m, &mut st, Facing::Front, 5),
            DamageOutcome::Destroyed
        );
        assert!(is_destroyed(&m, &st));
    }

    #[test]
    fn rear_armor_overflows_to_shared_internal() {
        let m = max(10, 4, 5);
        let mut st = LocState::default();
        // Rear has 4; 6 damage -> 4 rear, 2 internal
        assert_eq!(
            apply_damage(&m, &mut st, Facing::Rear, 6),
            DamageOutcome::IntoInternal(2)
        );
        assert_eq!(st.rear_hits, 4);
        assert_eq!(st.internal_hits, 2);
        // Front armor untouched
        assert_eq!(armor_remaining(&m, &st, Facing::Front), 10);
    }

    #[test]
    fn transfer_diagram() {
        use crate::domain::Location::*;
        assert_eq!(transfer_to(LeftArm), Some(LeftTorso));
        assert_eq!(transfer_to(RightLeg), Some(RightTorso));
        assert_eq!(transfer_to(LeftTorso), Some(CenterTorso));
        assert_eq!(transfer_to(CenterTorso), None);
        assert_eq!(transfer_to(Head), None);
        // Aerospace arcs all spill into the shared SI pool, which is terminal.
        assert_eq!(transfer_to(Nose), Some(AeroSI));
        assert_eq!(transfer_to(LeftWing), Some(AeroSI));
        assert_eq!(transfer_to(RightWing), Some(AeroSI));
        assert_eq!(transfer_to(Aft), Some(AeroSI));
        assert_eq!(transfer_to(AeroSI), None);
    }

    #[test]
    fn repair_restores_points() {
        let m = max(10, 0, 5);
        let mut st = LocState {
            armor_hits: 7,
            rear_hits: 0,
            internal_hits: 3,
        };
        repair_armor(&mut st, Facing::Front, 4);
        repair_internal(&mut st, 1);
        assert_eq!(armor_remaining(&m, &st, Facing::Front), 7);
        assert_eq!(internal_remaining(&m, &st), 3);
    }
}
