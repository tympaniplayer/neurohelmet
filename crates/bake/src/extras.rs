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

//! Hand-entered units that aren't in Mekbay's catalog (so they can't be baked from the API).
//!
//! Some official Alpha Strike units — gun emplacements and other Battlefield Support assets —
//! have printed AS cards but appear in neither Mekbay's `units.json` nor the MUL, and MegaMek's
//! `.blk` files for them carry no armor/AS stats (a gun emplacement's durability comes from the
//! building it occupies). The only complete source is the physical card, so we transcribe a few
//! into `data/extra_units.json` and merge them into the bundle at bake time. They are **AS-only**
//! (no Classic record sheet → empty `armor` map → [`Mech::is_as_only`]).

use neurohelmet_core::domain::{AsStats, HeatSinkType, Mech, MechConfig, UnitType};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// One hand-entered Alpha Strike unit, mirroring an official card. Only the fields an AS-only unit
/// needs; everything else on [`Mech`] is left empty/zero. The `as` object deserializes straight
/// into [`AsStats`] (use its field names: `pv`, `size`, `tp`, `movement`, `tmm`, `armor`,
/// `structure`, `dmg_s`/`dmg_m`/`dmg_l`/`dmg_e`, `overheat`, `specials`).
#[derive(Deserialize)]
struct ExtraUnit {
    chassis: String,
    model: String,
    #[serde(default)]
    unit_type: UnitType,
    #[serde(default)]
    subtype: String,
    #[serde(default)]
    tech_base: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    year: u16,
    #[serde(default)]
    internal: u16,
    #[serde(default)]
    walk: u8,
    #[serde(default)]
    run: u8,
    #[serde(default)]
    jump: u8,
    #[serde(rename = "as")]
    as_stats: AsStats,
}

/// Build a minimal AS-only [`Mech`] from a hand-entered card. The empty `armor` map is what marks
/// it AS-only ([`Mech::is_as_only`]).
fn build_extra(e: ExtraUnit) -> Mech {
    Mech {
        chassis: e.chassis,
        model: e.model,
        tonnage: 0,
        tech_base: e.tech_base,
        role: e.role,
        weight_class: String::new(),
        subtype: e.subtype,
        year: e.year,
        bv: 0,
        cost: 0,
        armor_type: String::new(),
        structure_type: String::new(),
        walk: e.walk,
        run: e.run,
        jump: e.jump,
        heat_sinks: 0,
        heat_sink_type: HeatSinkType::default(),
        dissipation: 0,
        config: MechConfig::default(),
        unit_type: e.unit_type,
        motive: None,
        internal: e.internal,
        dpt: 0,
        transport: Vec::new(),
        armor: BTreeMap::new(),
        weapons: Vec::new(),
        ammo: Vec::new(),
        equipment: Vec::new(),
        crit_slots: BTreeMap::new(),
        as_stats: e.as_stats,
        availability: BTreeMap::new(),
    }
}

/// Load the hand-entered units from `path` (e.g. `data/extra_units.json`). Returns an empty list
/// if the file is absent; panics on malformed JSON (a bake-time authoring error worth surfacing).
pub fn load_extra_units(path: &Path) -> Vec<Mech> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let extras: Vec<ExtraUnit> = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    extras.into_iter().map(build_extra).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(json: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn absent_file_yields_empty_list() {
        // A non-existent path is not an error — it just means "no extras".
        let mechs = load_extra_units(Path::new("/nonexistent/extra_units.json"));
        assert!(mechs.is_empty());
    }

    #[test]
    fn loads_an_as_only_unit_with_its_card_stats() {
        let json = r#"[
            {
                "chassis": "Gun Emplacement",
                "model": "Heavy",
                "unit_type": "BattleArmor",
                "subtype": "Battlefield Support",
                "tech_base": "Inner Sphere",
                "role": "Ambusher",
                "year": 3025,
                "internal": 6,
                "walk": 0,
                "run": 0,
                "jump": 0,
                "as": {
                    "pv": 18,
                    "size": 2,
                    "tp": "BS",
                    "movement": "0\"",
                    "tmm": 0,
                    "armor": 4,
                    "structure": 0,
                    "dmg_s": "3",
                    "dmg_m": "3",
                    "dmg_l": "2",
                    "dmg_e": "0",
                    "overheat": 0,
                    "specials": ["TUR"]
                }
            }
        ]"#;
        let f = write_temp(json);
        let mechs = load_extra_units(f.path());
        assert_eq!(mechs.len(), 1);
        let m = &mechs[0];
        // Card fields carried through.
        assert_eq!(m.chassis, "Gun Emplacement");
        assert_eq!(m.model, "Heavy");
        assert_eq!(m.year, 3025);
        assert_eq!(m.internal, 6);
        assert_eq!(m.as_stats.pv, 18);
        assert_eq!(m.as_stats.armor, 4);
        assert_eq!(m.as_stats.specials, vec!["TUR".to_string()]);
        // The empty armor map is exactly what marks it AS-only.
        assert!(m.armor.is_empty());
        assert!(m.is_as_only());
        // Classic-only fields default to empty/zero.
        assert_eq!(m.tonnage, 0);
        assert!(m.weapons.is_empty());
        assert!(m.crit_slots.is_empty());
    }

    #[test]
    fn optional_fields_default_when_omitted() {
        // Only the required chassis/model/as fields; everything #[serde(default)] omitted.
        let json = r#"[
            {"chassis":"Minimal","model":"X","as":{
                "pv":1,"size":1,"tp":"BM","movement":"0\"","tmm":0,"armor":0,"structure":0,
                "dmg_s":"0","dmg_m":"0","dmg_l":"0","dmg_e":"0","overheat":0,"specials":[]
            }}
        ]"#;
        let f = write_temp(json);
        let mechs = load_extra_units(f.path());
        assert_eq!(mechs.len(), 1);
        let m = &mechs[0];
        assert_eq!(m.year, 0);
        assert_eq!(m.tech_base, "");
        assert_eq!(m.walk, 0);
        assert!(m.is_as_only());
    }

    #[test]
    fn empty_array_yields_no_units() {
        let f = write_temp("[]");
        assert!(load_extra_units(f.path()).is_empty());
    }

    #[test]
    #[should_panic(expected = "parse")]
    fn malformed_json_panics_to_surface_authoring_errors() {
        let f = write_temp(r#"[{"chassis":"oops""#);
        let _ = load_extra_units(f.path());
    }
}
