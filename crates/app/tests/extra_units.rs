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

//! Integration check: the hand-entered AS-only units (data/extra_units.json) made it into the
//! baked bundle and look right. Guards against a re-bake that forgets to merge them.

use neurohelmet_core::data::bundle::Bundle;
use std::path::Path;

fn bundle() -> Bundle {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/mechs.bin");
    Bundle::load(Path::new(path)).expect("load baked bundle")
}

#[test]
fn extra_emplacements_and_foot_point_are_baked() {
    let b = bundle();
    let find = |name: &str| {
        b.mechs
            .iter()
            .find(|m| m.display_name() == name)
            .unwrap_or_else(|| panic!("missing baked extra unit: {name}"))
    };

    // Gun emplacements: AS-only (no Classic armor), with the card's PV/damage.
    let heavy = find("Heavy Emplacement (AC/20)");
    assert!(heavy.is_as_only(), "emplacement has no Classic record sheet");
    assert_eq!(heavy.as_stats.pv, 15);
    assert_eq!(heavy.as_stats.tp, "BD");
    assert_eq!(heavy.as_stats.dmg_s, "2");
    assert_eq!(heavy.as_stats.armor, 6);
    assert!(heavy.as_stats.specials.iter().any(|s| s == "IMMOBILE"));

    find("Medium Emplacement (2x AC/5)");
    let light = find("Light Emplacement (3x AC/2)");
    assert_eq!(light.as_stats.dmg_s, "0*"); // minimal-damage notation survives

    // The Clan Foot Point energy variant Mekbay lacks (it only has the ballistic "Rifle Advanced").
    let foot = find("Clan Foot Point (Rifle, Energy) Advanced");
    assert!(foot.is_infantry());
    assert!(foot.is_as_only());
    assert_eq!(foot.as_stats.pv, 11);
    assert_eq!(foot.as_stats.dmg_m, "1"); // energy rifles reach Medium (ballistic twin is 0)
}
