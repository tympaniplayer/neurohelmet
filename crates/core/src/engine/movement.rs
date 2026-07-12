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

//! Movement-derived to-hit modifiers (Total Warfare): the attacker's own movement modifier
//! and the Target Movement Modifier from hexes moved. Pure tables — what a unit *did* this
//! turn lives on `TrackedMech`.

use serde::{Deserialize, Serialize};

/// How a unit moved this turn. Tracked per turn (cleared on end-turn); the player sets it by
/// hand after the movement phase. Vehicles read `Walked`/`Ran` as Cruised/Flanked.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum MoveMode {
    #[default]
    Stationary,
    Walked,
    Ran,
    Jumped,
}

impl MoveMode {
    /// Cycle order for the UI selector.
    pub const ALL: [MoveMode; 4] = [
        MoveMode::Stationary,
        MoveMode::Walked,
        MoveMode::Ran,
        MoveMode::Jumped,
    ];

    /// UI label; vehicles move at Cruise/Flank instead of Walk/Run, and infantry have a single
    /// ground speed (no run) so any ground move just reads "moved".
    pub fn label(self, vehicle: bool, infantry: bool) -> &'static str {
        match (self, vehicle, infantry) {
            (MoveMode::Stationary, _, _) => "stationary",
            (MoveMode::Walked | MoveMode::Ran, _, true) => "moved",
            (MoveMode::Walked, false, _) => "walked",
            (MoveMode::Walked, true, _) => "cruised",
            (MoveMode::Ran, false, _) => "ran",
            (MoveMode::Ran, true, _) => "flanked",
            (MoveMode::Jumped, _, _) => "jumped",
        }
    }
}

/// The attacker's to-hit modifier from its own movement this turn (TW: walked +1, ran +2,
/// jumped +3; vehicles take the same values for cruise/flank).
pub fn attacker_movement_modifier(mode: MoveMode) -> i32 {
    match mode {
        MoveMode::Stationary => 0,
        MoveMode::Walked => 1,
        MoveMode::Ran => 2,
        MoveMode::Jumped => 3,
    }
}

/// Target Movement Modifier from hexes moved this turn (TW table), plus +1 if the target
/// jumped. This is the modifier *opponents* take when shooting at this unit.
pub fn target_movement_modifier(hexes: u8, mode: MoveMode) -> i32 {
    let base = match hexes {
        0..=2 => 0,
        3..=4 => 1,
        5..=6 => 2,
        7..=9 => 3,
        10..=17 => 4,
        18..=24 => 5,
        _ => 6,
    };
    base + i32::from(mode == MoveMode::Jumped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attacker_modifier_table() {
        assert_eq!(attacker_movement_modifier(MoveMode::Stationary), 0);
        assert_eq!(attacker_movement_modifier(MoveMode::Walked), 1);
        assert_eq!(attacker_movement_modifier(MoveMode::Ran), 2);
        assert_eq!(attacker_movement_modifier(MoveMode::Jumped), 3);
    }

    #[test]
    fn tmm_brackets() {
        assert_eq!(target_movement_modifier(0, MoveMode::Stationary), 0);
        assert_eq!(target_movement_modifier(2, MoveMode::Walked), 0);
        assert_eq!(target_movement_modifier(3, MoveMode::Walked), 1);
        assert_eq!(target_movement_modifier(4, MoveMode::Ran), 1);
        assert_eq!(target_movement_modifier(5, MoveMode::Ran), 2);
        assert_eq!(target_movement_modifier(7, MoveMode::Ran), 3);
        assert_eq!(target_movement_modifier(10, MoveMode::Ran), 4);
        assert_eq!(target_movement_modifier(18, MoveMode::Ran), 5);
        assert_eq!(target_movement_modifier(25, MoveMode::Ran), 6);
    }

    #[test]
    fn jumping_adds_one_to_tmm() {
        assert_eq!(target_movement_modifier(5, MoveMode::Jumped), 3);
        assert_eq!(target_movement_modifier(0, MoveMode::Jumped), 1);
    }

    #[test]
    fn vehicle_labels() {
        assert_eq!(MoveMode::Walked.label(true, false), "cruised");
        assert_eq!(MoveMode::Ran.label(true, false), "flanked");
        assert_eq!(MoveMode::Ran.label(false, false), "ran");
        // Infantry have one ground speed — any ground move is just "moved".
        assert_eq!(MoveMode::Walked.label(false, true), "moved");
        assert_eq!(MoveMode::Ran.label(false, true), "moved");
        assert_eq!(MoveMode::Jumped.label(false, true), "jumped");
    }
}
