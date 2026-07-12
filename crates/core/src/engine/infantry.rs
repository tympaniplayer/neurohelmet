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

//! Conventional-infantry weapon range rules (Total Warfare). A platoon's primary weapon has a
//! *range class* (1–7; MegaMek's `InfantryWeapon.infantryRange`), not a fixed hex range — the hex
//! reach is `class × 3`, and the to-hit modifier per hex comes from the Conventional Infantry Range
//! Modifier Table. Transcribed from MegaMek `Compute.getInfantryRangeMods`.

/// Maximum hex range a conventional-infantry weapon of this range class reaches (`class × 3`).
pub fn infantry_max_range(range_class: u8) -> u8 {
    range_class.saturating_mul(3)
}

/// The "infantry range" to-hit modifier for a conventional-infantry weapon of the given range class
/// firing at `distance` hexes (MegaMek `Compute.getInfantryRangeMods`, the base modifier — the
/// flag-based point-blank / burst / encumber tweaks at distance 0 are the player's). `None` = the
/// target is out of range.
pub fn infantry_range_mod(range_class: u8, distance: u8) -> Option<i32> {
    let d = distance as i32;
    let m = match range_class {
        0 => return (d == 0).then_some(0),
        1 => match d {
            0 => -2,
            1 => 0,
            2 => 2,
            3 => 4,
            _ => return None,
        },
        2 if d <= 6 => match d {
            0 => -2,
            d if d > 4 => 4,
            d if d > 2 => 2,
            _ => 0,
        },
        3 if d <= 9 => match d {
            0 => -2,
            d if d > 6 => 4,
            d if d > 3 => 2,
            _ => 0,
        },
        4 if d <= 12 => match d {
            0 => -2,
            d if d > 10 => 4,
            d if d > 8 => 3,
            d if d > 6 => 2,
            d if d > 4 => 1,
            _ => 0,
        },
        5 if d <= 15 => match d {
            0 => -1,
            d if d > 12 => 4,
            d if d > 10 => 3,
            d if d > 7 => 2,
            d if d > 5 => 1,
            _ => 0,
        },
        6 if d <= 18 => match d {
            0 => -1,
            d if d > 15 => 5,
            d if d > 12 => 4,
            d if d > 9 => 2,
            d if d > 6 => 1,
            _ => 0,
        },
        7 if d <= 21 => match d {
            0 => -1,
            d if d > 17 => 6,
            d if d > 14 => 4,
            d if d > 10 => 2,
            d if d > 7 => 1,
            _ => 0,
        },
        _ => return None,
    };
    Some(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_range_is_three_per_class() {
        assert_eq!(infantry_max_range(1), 3);
        assert_eq!(infantry_max_range(2), 6);
        assert_eq!(infantry_max_range(3), 9);
    }

    #[test]
    fn machine_gun_class_one_matches_the_table() {
        // Conventional Infantry Range Modifier Table, Machine Gun / Rifle (Ballistic) row.
        let row: Vec<_> = (0..=4).map(|d| infantry_range_mod(1, d)).collect();
        assert_eq!(row, vec![Some(-2), Some(0), Some(2), Some(4), None]);
    }

    #[test]
    fn class_two_and_three_brackets() {
        // Rifle (Energy) / SRM row (class 2): 0:-2, 1-2:0, 3-4:+2, 5-6:+4, 7:out.
        let two: Vec<_> = (0..=7).map(|d| infantry_range_mod(2, d)).collect();
        assert_eq!(
            two,
            vec![
                Some(-2),
                Some(0),
                Some(0),
                Some(2),
                Some(2),
                Some(4),
                Some(4),
                None
            ]
        );
        // LRM row (class 3): 0:-2, 1-3:0, 4-6:+2, 7-9:+4, 10:out.
        let three: Vec<_> = (0..=10).map(|d| infantry_range_mod(3, d)).collect();
        assert_eq!(
            three,
            vec![
                Some(-2),
                Some(0),
                Some(0),
                Some(0),
                Some(2),
                Some(2),
                Some(2),
                Some(4),
                Some(4),
                Some(4),
                None,
            ],
        );
    }

    #[test]
    fn class_zero_only_reaches_its_own_hex() {
        // A range-class-0 weapon (e.g. a vibroblade) only hits at distance 0.
        assert_eq!(infantry_range_mod(0, 0), Some(0));
        assert_eq!(infantry_range_mod(0, 1), None);
        assert_eq!(infantry_range_mod(0, 5), None);
        // And max_range follows class × 3 = 0.
        assert_eq!(infantry_max_range(0), 0);
    }

    #[test]
    fn class_four_brackets() {
        // class 4 (max 12): 0:-2, 1-4:0, 5-6:+1, 7-8:+2, 9-10:+3, 11-12:+4, 13:out.
        let row: Vec<_> = (0..=13).map(|d| infantry_range_mod(4, d)).collect();
        assert_eq!(
            row,
            vec![
                Some(-2),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(1),
                Some(1),
                Some(2),
                Some(2),
                Some(3),
                Some(3),
                Some(4),
                Some(4),
                None,
            ],
        );
    }

    #[test]
    fn class_five_brackets() {
        // class 5 (max 15): 0:-1, 1-5:0, 6-7:+1, 8-10:+2, 11-12:+3, 13-15:+4, 16:out.
        // Note the point-blank modifier softens to -1 from class 5 up.
        let row: Vec<_> = (0..=16).map(|d| infantry_range_mod(5, d)).collect();
        assert_eq!(
            row,
            vec![
                Some(-1),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(1),
                Some(1),
                Some(2),
                Some(2),
                Some(2),
                Some(3),
                Some(3),
                Some(4),
                Some(4),
                Some(4),
                None,
            ],
        );
    }

    #[test]
    fn class_six_brackets() {
        // class 6 (max 18): 0:-1, 1-6:0, 7-9:+1, 10-12:+2, 13-15:+4, 16-18:+5, 19:out.
        let row: Vec<_> = (0..=19).map(|d| infantry_range_mod(6, d)).collect();
        assert_eq!(
            row,
            vec![
                Some(-1),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(1),
                Some(1),
                Some(1),
                Some(2),
                Some(2),
                Some(2),
                Some(4),
                Some(4),
                Some(4),
                Some(5),
                Some(5),
                Some(5),
                None,
            ],
        );
    }

    #[test]
    fn class_seven_brackets() {
        // class 7 (max 21): 0:-1, 1-7:0, 8-10:+1, 11-14:+2, 15-17:+4, 18-21:+6, 22:out.
        let row: Vec<_> = (0..=22).map(|d| infantry_range_mod(7, d)).collect();
        assert_eq!(
            row,
            vec![
                Some(-1),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(0),
                Some(1),
                Some(1),
                Some(1),
                Some(2),
                Some(2),
                Some(2),
                Some(2),
                Some(4),
                Some(4),
                Some(4),
                Some(6),
                Some(6),
                Some(6),
                Some(6),
                None,
            ],
        );
    }

    #[test]
    fn unknown_class_is_always_out_of_range() {
        // class 8+ has no table row → never in range, at any distance.
        assert_eq!(infantry_range_mod(8, 0), None);
        assert_eq!(infantry_range_mod(8, 3), None);
        assert_eq!(infantry_range_mod(255, 0), None);
    }

    #[test]
    fn max_range_saturates_instead_of_overflowing() {
        assert_eq!(infantry_max_range(7), 21);
        // class × 3 would overflow u8 past 85; saturate rather than wrap.
        assert_eq!(infantry_max_range(85), 255);
        assert_eq!(infantry_max_range(200), 255);
    }
}
