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

//! MechWarrior (pilot) damage: the 6-box hit track and consciousness avoid numbers
//! (Total Warfare). Pure rules, no UI/IO.

/// A pilot is killed after this many hits.
pub const PILOT_MAX: u8 = 6;

/// The 2d6 target a pilot must roll to stay conscious after taking `hits` total damage.
///
/// `None` means no roll is needed: either the pilot is unhurt (`0`) or already dead
/// (`>= PILOT_MAX`). Standard table: 1→3+, 2→5+, 3→7+, 4→10+, 5→11+.
pub fn consciousness_avoid(hits: u8) -> Option<u8> {
    match hits {
        1 => Some(3),
        2 => Some(5),
        3 => Some(7),
        4 => Some(10),
        5 => Some(11),
        _ => None, // 0 = unhurt, >=6 = dead
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consciousness_numbers_climb_with_hits() {
        assert_eq!(consciousness_avoid(0), None);
        assert_eq!(consciousness_avoid(1), Some(3));
        assert_eq!(consciousness_avoid(2), Some(5));
        assert_eq!(consciousness_avoid(3), Some(7));
        assert_eq!(consciousness_avoid(4), Some(10));
        assert_eq!(consciousness_avoid(5), Some(11));
        // At and beyond the max the pilot is dead — no roll.
        assert_eq!(consciousness_avoid(PILOT_MAX), None);
        assert_eq!(consciousness_avoid(9), None);
    }
}
