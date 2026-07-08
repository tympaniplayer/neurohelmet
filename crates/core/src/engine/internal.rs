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

//! Standard BattleTech internal-structure-by-tonnage table.
//!
//! At runtime the maxima come from the baked data (the SVG carries internal pips directly),
//! so this table is used for validation and to support a future MTF-only bake path.

use crate::domain::Location;

/// `(tons, center_torso, side_torso, arm, leg)` for standard biped 'Mechs, 20–100t.
const TABLE: &[(u16, u16, u16, u16, u16)] = &[
    (20, 6, 5, 3, 4),
    (25, 8, 6, 4, 6),
    (30, 10, 7, 5, 7),
    (35, 11, 8, 6, 8),
    (40, 12, 10, 6, 10),
    (45, 14, 11, 7, 11),
    (50, 16, 12, 8, 12),
    (55, 18, 13, 9, 13),
    (60, 20, 14, 10, 14),
    (65, 21, 15, 10, 15),
    (70, 22, 15, 11, 15),
    (75, 23, 16, 12, 16),
    (80, 25, 17, 13, 17),
    (85, 27, 18, 14, 18),
    (90, 29, 19, 15, 19),
    (95, 30, 20, 16, 20),
    (100, 31, 21, 17, 21),
];

/// Internal structure points for `loc` on a standard `tons`-ton biped. Head is always 3.
/// Returns 0 for tonnages outside the standard 20–100 table.
pub fn internal_structure(tons: u16, loc: Location) -> u16 {
    if loc == Location::Head {
        return 3;
    }
    let Some(&(_, ct, side, arm, leg)) = TABLE.iter().find(|(t, ..)| *t == tons) else {
        return 0;
    };
    use Location::*;
    match loc {
        CenterTorso => ct,
        LeftTorso | RightTorso => side,
        LeftArm | RightArm => arm,
        // Every leg (biped, quad, or tripod center) uses the leg value.
        LeftLeg | RightLeg | FrontLeftLeg | FrontRightLeg | RearLeftLeg | RearRightLeg
        | CenterLeg => leg,
        Head => 3,
        // Vehicle locations: this 'Mech internal-structure table doesn't apply.
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_100t() {
        assert_eq!(internal_structure(100, Location::Head), 3);
        assert_eq!(internal_structure(100, Location::CenterTorso), 31);
        assert_eq!(internal_structure(100, Location::LeftTorso), 21);
        assert_eq!(internal_structure(100, Location::RightArm), 17);
        assert_eq!(internal_structure(100, Location::LeftLeg), 21);
    }

    #[test]
    fn locust_20t() {
        assert_eq!(internal_structure(20, Location::CenterTorso), 6);
        assert_eq!(internal_structure(20, Location::LeftTorso), 5);
        assert_eq!(internal_structure(20, Location::LeftArm), 3);
        assert_eq!(internal_structure(20, Location::RightLeg), 4);
    }

    #[test]
    fn unknown_tonnage_returns_zero() {
        assert_eq!(internal_structure(7, Location::CenterTorso), 0);
    }
}
