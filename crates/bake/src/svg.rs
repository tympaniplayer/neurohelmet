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

//! Parse a Mekbay record-sheet SVG into per-location armor/internal maxima.
//!
//! Every pip in the sheet is a tagged element, e.g.
//! `<circle loc="CT" rear="1" class="pip armor" .../>` or `class="pip structure"`.
//! Counting pips by `(loc, kind, rear)` reconstructs the record sheet exactly (verified
//! against the canonical Atlas AS7-D during planning).

use neurohelmet_core::domain::{CritSlot, Location, LocationArmor};
use std::collections::BTreeMap;

/// Count pips in `svg_text`, grouped into a per-location armor table.
pub fn parse_armor(svg_text: &str) -> Result<BTreeMap<Location, LocationArmor>, String> {
    let doc = roxmltree::Document::parse(svg_text).map_err(|e| format!("svg parse: {e}"))?;
    let mut table: BTreeMap<Location, LocationArmor> = BTreeMap::new();

    for node in doc.descendants() {
        if !node.is_element() {
            continue;
        }
        let class = node.attribute("class").unwrap_or("");
        if !class.contains("pip") {
            continue;
        }
        let is_armor = class.contains("armor");
        let is_structure = class.contains("structure");
        if !is_armor && !is_structure {
            continue;
        }
        let Some(loc) = node.attribute("loc").and_then(Location::from_code) else {
            continue;
        };
        let rear = node.attribute("rear").is_some();
        let entry = table.entry(loc).or_default();
        if is_structure {
            entry.internal_max += 1;
        } else if rear {
            entry.rear_max += 1;
        } else {
            entry.armor_max += 1;
        }
    }

    if table.is_empty() {
        return Err("no pips found in sheet".into());
    }
    Ok(table)
}

/// The aerospace fighter's `(heat_sinks, dissipation)` from the sheet's `id="hsCount"` text. The
/// text is `"10"` for single sinks (10 sinks → 10 dissipation) or `"16 (32)"` for doubles (16 sinks
/// → 32 dissipation). Returns `(0, 0)` if absent/unparseable.
pub fn parse_aero_heat(svg_text: &str) -> (u16, u16) {
    let Ok(doc) = roxmltree::Document::parse(svg_text) else {
        return (0, 0);
    };
    for node in doc.descendants() {
        if node.attribute("id") == Some("hsCount") {
            // The text may sit on the node or in child <tspan>s; gather only the text nodes (the
            // element node also reports its child's text, which would double-count).
            let txt: String = node
                .descendants()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect();
            let nums: Vec<u16> = txt
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|s| s.parse().ok())
                .collect();
            return match nums.as_slice() {
                [sinks] => (*sinks, *sinks),          // singles: dissipation == count
                [sinks, diss, ..] => (*sinks, *diss), // doubles: "N (2N)"
                _ => (0, 0),
            };
        }
    }
    (0, 0)
}

/// Transport / storage capacity from a vehicle (or other) record sheet's "Features …" line, e.g.
/// `Features Cargo (8 tons), Infantry Compartment (4 tons)`. Returns the comma-separated entries
/// that name a transport or storage bay (Infantry Compartment, Cargo, Troop/Infantry Bay, …) with
/// their tonnage, dropping non-storage features (Chassis Mods, etc.). Empty when the sheet has none.
pub fn parse_transport(svg_text: &str) -> Vec<String> {
    const KEYWORDS: [&str; 6] = [
        "Compartment",
        "Cargo",
        "Bay",
        "Troop",
        "Seating",
        "Quarters",
    ];
    let Ok(doc) = roxmltree::Document::parse(svg_text) else {
        return Vec::new();
    };
    for node in doc.descendants() {
        if !node.is_element() || node.tag_name().name() != "text" {
            continue;
        }
        let txt: String = node
            .descendants()
            .filter(|n| n.is_text())
            .filter_map(|n| n.text())
            .collect();
        let Some(list) = txt.trim().strip_prefix("Features ") else {
            continue;
        };
        let entries: Vec<String> = list
            .split(',')
            .map(str::trim)
            .filter(|e| KEYWORDS.iter().any(|k| e.contains(k)))
            .map(str::to_string)
            .collect();
        if !entries.is_empty() {
            return entries;
        }
    }
    Vec::new()
}

/// Count pips in a Battle Armor sheet: each trooper (`loc="T1".."T6"`) has one row of
/// `pip armor` circles where the **dark-filled** pip (`fill="#3f3f3f"`) is the trooper
/// himself (1 structure) and the white pips are suit armor. (Verified against the Achileus
/// 6+1×4 and Elemental 10+1×4 sheets.)
pub fn parse_ba_armor(svg_text: &str) -> Result<BTreeMap<Location, LocationArmor>, String> {
    let doc = roxmltree::Document::parse(svg_text).map_err(|e| format!("svg parse: {e}"))?;
    let mut table: BTreeMap<Location, LocationArmor> = BTreeMap::new();

    for node in doc.descendants() {
        if !node.is_element() {
            continue;
        }
        let class = node.attribute("class").unwrap_or("");
        if !class.contains("pip") || !class.contains("armor") {
            continue;
        }
        let Some(loc) = node.attribute("loc").and_then(Location::from_code) else {
            continue;
        };
        let entry = table.entry(loc).or_default();
        if node.attribute("fill") == Some("#3f3f3f") {
            entry.internal_max += 1;
        } else {
            entry.armor_max += 1;
        }
    }

    if table.is_empty() {
        return Err("no trooper pips found in sheet".into());
    }
    Ok(table)
}

/// Parse the critical-hit slot tables. Each slot is a `<g class="critSlot">` carrying
/// `loc`, `slot`, `name`, `type` (`sys`/`eq`), and `hittable`. Slots are grouped per
/// location and returned in slot order. Pseudo-rows without a real location (e.g. the
/// physical-attack list, `loc="—"`) are skipped via [`Location::from_code`].
pub fn parse_crit_slots(svg_text: &str) -> Result<BTreeMap<Location, Vec<CritSlot>>, String> {
    let doc = roxmltree::Document::parse(svg_text).map_err(|e| format!("svg parse: {e}"))?;
    let mut table: BTreeMap<Location, Vec<CritSlot>> = BTreeMap::new();

    for node in doc.descendants() {
        if !node.is_element() {
            continue;
        }
        let class = node.attribute("class").unwrap_or("");
        if !class.split_whitespace().any(|c| c == "critSlot") {
            continue;
        }
        let Some(loc) = node.attribute("loc").and_then(Location::from_code) else {
            continue;
        };
        let Some(slot) = node.attribute("slot").and_then(|s| s.parse::<u8>().ok()) else {
            continue;
        };
        let name = node.attribute("name").unwrap_or("").to_string();
        let system = node.attribute("type") == Some("sys");
        let hittable = node.attribute("hittable") == Some("1");
        // `uid` is shared by every slot of one physical item (both slots of a double heat
        // sink — but also all three slots of an engine, so deduping by uid is only valid
        // where one item = one effect). `hs` is the dissipation per sink (0 otherwise).
        // Only sink slots keep their uid: that's all the runtime needs, and baking every
        // slot's uid would grow the bundle by ~50%.
        let hs = node
            .attribute("hs")
            .and_then(|s| s.parse::<u8>().ok())
            .unwrap_or(0);
        let uid = if hs > 0 {
            node.attribute("uid").unwrap_or("").to_string()
        } else {
            String::new()
        };
        table.entry(loc).or_default().push(CritSlot {
            slot,
            name,
            system,
            hittable,
            uid,
            hs,
        });
    }

    for slots in table.values_mut() {
        slots.sort_by_key(|c| c.slot);
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aero_heat_singles_and_doubles() {
        let single =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text id="hsCount">10</text></svg>"#;
        assert_eq!(parse_aero_heat(single), (10, 10));
        let double =
            r#"<svg xmlns="http://www.w3.org/2000/svg"><text id="hsCount">16 (32)</text></svg>"#;
        assert_eq!(parse_aero_heat(double), (16, 32));
        let none = r#"<svg xmlns="http://www.w3.org/2000/svg"></svg>"#;
        assert_eq!(parse_aero_heat(none), (0, 0));
    }

    #[test]
    fn counts_pips_by_location_and_kind() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg">
            <circle loc="HD" class="pip armor"/>
            <circle loc="HD" class="pip armor"/>
            <circle loc="HD" class="pip structure"/>
            <circle loc="CT" class="pip armor"/>
            <circle loc="CT" rear="1" class="pip armor"/>
            <circle loc="CT" class="pip structure"/>
            <circle loc="XX" class="pip armor"/>
            <circle class="someOther thing"/>
        </svg>"#;
        let t = parse_armor(svg).unwrap();
        let hd = t[&Location::Head];
        assert_eq!(hd.armor_max, 2);
        assert_eq!(hd.internal_max, 1);
        assert_eq!(hd.rear_max, 0);
        let ct = t[&Location::CenterTorso];
        assert_eq!(ct.armor_max, 1);
        assert_eq!(ct.rear_max, 1);
        assert_eq!(ct.internal_max, 1);
        // "XX" is not a valid location and is ignored.
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn real_atlas_sheet_matches_canonical_armor() {
        // The committed fixture is the actual Mekbay Atlas AS7-D record sheet.
        let svg = include_str!("../tests/fixtures/Atlas_AS7-D.svg");
        let t = parse_armor(svg).unwrap();
        let expect = |loc: Location, armor: u16, rear: u16, internal: u16| {
            let la = t[&loc];
            assert_eq!(la.armor_max, armor, "{} armor", loc.code());
            assert_eq!(la.rear_max, rear, "{} rear", loc.code());
            assert_eq!(la.internal_max, internal, "{} internal", loc.code());
        };
        expect(Location::Head, 9, 0, 3);
        expect(Location::CenterTorso, 47, 14, 31);
        expect(Location::LeftTorso, 32, 10, 21);
        expect(Location::RightTorso, 32, 10, 21);
        expect(Location::LeftArm, 34, 0, 17);
        expect(Location::RightArm, 34, 0, 17);
        expect(Location::LeftLeg, 41, 0, 21);
        expect(Location::RightLeg, 41, 0, 21);

        let armor_total: u16 = t.values().map(|l| l.armor_max + l.rear_max).sum();
        let internal_total: u16 = t.values().map(|l| l.internal_max).sum();
        assert_eq!(armor_total, 304);
        assert_eq!(internal_total, 152);
    }

    #[test]
    fn transport_features_filtered_to_storage() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <text x="8" y="25">Features Chassis Mod: Off-Road, Cargo (8 tons), Infantry Compartment (4 tons)</text>
        </svg>"##;
        assert_eq!(
            parse_transport(svg),
            vec![
                "Cargo (8 tons)".to_string(),
                "Infantry Compartment (4 tons)".to_string()
            ],
        );
        // No Features line, or only non-storage features → empty.
        let none = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <text>Features Chassis Mod: Off-Road</text></svg>"##;
        assert!(parse_transport(none).is_empty());
        assert!(parse_transport("<svg xmlns=\"http://www.w3.org/2000/svg\"/>").is_empty());
    }

    #[test]
    fn ba_sheet_pips_split_armor_and_trooper() {
        // Per trooper: the dark-filled pip is the trooper (structure), white pips are armor.
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <circle fill="#3f3f3f" loc="T1" class="pip armor"/>
            <circle fill="#fff" loc="T1" class="pip armor"/>
            <circle fill="#fff" loc="T1" class="pip armor"/>
            <circle fill="#3f3f3f" loc="T2" class="pip armor"/>
            <circle fill="#fff" loc="T2" class="pip armor"/>
        </svg>"##;
        let t = parse_ba_armor(svg).unwrap();
        assert_eq!(t[&Location::Trooper1].armor_max, 2);
        assert_eq!(t[&Location::Trooper1].internal_max, 1);
        assert_eq!(t[&Location::Trooper2].armor_max, 1);
        assert_eq!(t[&Location::Trooper2].internal_max, 1);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn real_ba_sheet_armor() {
        // The committed Atlas fixture is a 'Mech — just confirm the BA parser rejects it
        // cleanly rather than mis-counting (no T1..T6 locs).
        let svg = include_str!("../tests/fixtures/Atlas_AS7-D.svg");
        // Mech sheets do carry pip-armor elements, but none with trooper locs; they'd land in
        // mech locations. parse_ba_armor is only called for Battle Armor subtypes.
        let t = parse_ba_armor(svg);
        assert!(t.is_ok_and(|t| !t.contains_key(&Location::Trooper1)));
    }

    #[test]
    fn counts_crit_slots_by_location() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg">
            <g loc="HD" class="critGroup">
              <g type="sys" slot="0" loc="HD" class="critSlot" name="Life Support" hittable="1"/>
              <g type="eq" slot="3" loc="HD" uid="Heat Sink@HD#3" hs="1" class="critSlot" name="Heat Sink" hittable="1"/>
            </g>
            <g type="eq" slot="1" loc="—" class="critSlot" name="Kick" hittable="0"/>
            <g class="someOther thing"/>
        </svg>"#;
        let t = parse_crit_slots(svg).unwrap();
        let hd = &t[&Location::Head];
        assert_eq!(hd.len(), 2, "two HD slots; the loc=— pseudo-row is skipped");
        assert_eq!(hd[0].slot, 0);
        assert_eq!(hd[0].name, "Life Support");
        assert!(hd[0].system);
        assert_eq!(hd[0].uid, "", "no uid attribute -> empty");
        assert_eq!(hd[0].hs, 0, "not a heat sink");
        assert_eq!(hd[1].name, "Heat Sink");
        assert!(!hd[1].system, "equipment, not a system");
        assert_eq!(hd[1].uid, "Heat Sink@HD#3");
        assert_eq!(hd[1].hs, 1);
        assert_eq!(t.len(), 1, "only Head; the non-location row is dropped");
    }

    #[test]
    fn real_atlas_sheet_crit_slots() {
        let svg = include_str!("../tests/fixtures/Atlas_AS7-D.svg");
        let t = parse_crit_slots(svg).unwrap();
        // Head column, in order, matches the canonical sheet.
        let hd: Vec<&str> = t[&Location::Head].iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            hd,
            [
                "Life Support",
                "Sensors",
                "Cockpit",
                "Heat Sink",
                "Sensors",
                "Life Support"
            ]
        );
        // Center torso leads with engine + gyro systems.
        let ct = &t[&Location::CenterTorso];
        assert!(ct.iter().any(|c| c.name == "Fusion Engine" && c.system));
        assert!(ct.iter().any(|c| c.name == "Gyro" && c.system));
        // The head heat sink carries its grouping uid + per-sink dissipation.
        let hd_sink = t[&Location::Head]
            .iter()
            .find(|c| c.name == "Heat Sink")
            .unwrap();
        assert_eq!(hd_sink.uid, "Heat Sink@HD#3");
        assert_eq!(hd_sink.hs, 1);
        // All eight biped locations carry crit tables.
        assert_eq!(t.len(), 8);
    }
}
