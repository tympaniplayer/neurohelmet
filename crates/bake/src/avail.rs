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

//! Faction/era availability (§35). Fetches Mekbay's era + faction catalogs and the
//! RATGenerator-derived availability table, and joins them into:
//!   - `eras` / `factions` label maps (baked into the [`Bundle`])
//!   - a per-unit `availability` table keyed by the units.json `name` (e.g. `BMAtlas_AS7D`).
//!
//! The availability table records, per `era_id -> faction_id`, the rarity score
//! `max(requisition, salvage)` (1..=100). Only nonzero scores are kept; an absent entry means a
//! unit isn't available to that faction/era (or has no RAT data at all).
//!
//! Sources (all reproducible from a clean checkout):
//!   - `https://db.mekbay.com/eras.json`     — `{eras:[{id,name,years:{from,to}}]}`
//!   - `https://db.mekbay.com/factions.json` — `{factions:[{id,name,group}]}`
//!   - `https://mekbay.com/assets/mulized_availability_weighted.json` — `[{n,e:{era:{fac:[req,sal]}}}]`

use crate::fetch::Fetcher;
use neurohelmet_core::data::bundle::{EraInfo, FactionInfo};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The availability score table for one unit: `era_id -> faction_id -> max(req,salvage)`.
pub type UnitAvailability = BTreeMap<u16, BTreeMap<u16, u8>>;

pub struct AvailCatalogs {
    pub eras: Vec<EraInfo>,
    pub factions: Vec<FactionInfo>,
    /// Keyed by the units.json `name` field (the availability file's `n`).
    pub by_name: HashMap<String, UnitAvailability>,
}

const AVAIL_URL: &str = "https://mekbay.com/assets/mulized_availability_weighted.json";

/// Fetch + parse the era catalog, faction catalog, and availability table.
pub fn fetch(fetcher: &Fetcher) -> Result<AvailCatalogs, String> {
    let eras = parse_eras(&fetcher.get_text("eras.json").map_err(|e| format!("eras.json: {e}"))?)?;
    let factions =
        parse_factions(&fetcher.get_text("factions.json").map_err(|e| format!("factions.json: {e}"))?)?;
    let avail_text = fetcher
        .get_text_url(AVAIL_URL, "mulized_availability_weighted.json")
        .map_err(|e| format!("availability: {e}"))?;
    let known: HashSet<u16> = factions.iter().map(|f| f.id).collect();
    let by_name = parse_availability(&avail_text, &known)?;
    Ok(AvailCatalogs { eras, factions, by_name })
}

fn parse_eras(text: &str) -> Result<Vec<EraInfo>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("eras.json: {e}"))?;
    let arr = v.get("eras").and_then(Value::as_array).ok_or("eras.json: missing `eras`")?;
    let mut eras: Vec<EraInfo> = arr
        .iter()
        .filter_map(|e| {
            Some(EraInfo {
                id: u16::try_from(e.get("id")?.as_u64()?).ok()?,
                name: e.get("name")?.as_str()?.to_string(),
                from: e.get("years")?.get("from")?.as_u64()? as u16,
                to: e.get("years")?.get("to")?.as_u64().unwrap_or(9999).min(9999) as u16,
            })
        })
        .collect();
    eras.sort_by_key(|e| e.from);
    if eras.is_empty() {
        return Err("eras.json: no eras parsed".into());
    }
    Ok(eras)
}

fn parse_factions(text: &str) -> Result<Vec<FactionInfo>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("factions.json: {e}"))?;
    let arr = v.get("factions").and_then(Value::as_array).ok_or("factions.json: missing `factions`")?;
    let mut factions: Vec<FactionInfo> = arr
        .iter()
        .filter_map(|f| {
            Some(FactionInfo {
                id: u16::try_from(f.get("id")?.as_u64()?).ok()?,
                name: f.get("name")?.as_str()?.to_string(),
                group: f.get("group").and_then(Value::as_str).unwrap_or("").to_string(),
            })
        })
        .collect();
    factions.sort_by(|a, b| (a.group.clone(), a.name.clone()).cmp(&(b.group.clone(), b.name.clone())));
    if factions.is_empty() {
        return Err("factions.json: no factions parsed".into());
    }
    Ok(factions)
}

/// Parse the availability array into `name -> (era -> faction -> score)`, dropping zero scores and
/// any faction id not present in `known` (e.g. the placeholder id 0).
fn parse_availability(text: &str, known: &HashSet<u16>) -> Result<HashMap<String, UnitAvailability>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("availability: {e}"))?;
    let arr = v.as_array().ok_or("availability: expected a top-level array")?;
    let mut out = HashMap::with_capacity(arr.len());
    for rec in arr {
        let Some(name) = rec.get("n").and_then(Value::as_str) else { continue };
        let Some(eras) = rec.get("e").and_then(Value::as_object) else { continue };
        let mut table: UnitAvailability = BTreeMap::new();
        for (era_key, fac_map) in eras {
            let Ok(era_id) = era_key.parse::<u16>() else { continue };
            let Some(fac_map) = fac_map.as_object() else { continue };
            let mut row: BTreeMap<u16, u8> = BTreeMap::new();
            for (fac_key, pair) in fac_map {
                let Ok(fac_id) = fac_key.parse::<u16>() else { continue };
                if !known.contains(&fac_id) {
                    continue;
                }
                let score = pair
                    .as_array()
                    .map(|a| {
                        let req = a.first().and_then(Value::as_u64).unwrap_or(0);
                        let sal = a.get(1).and_then(Value::as_u64).unwrap_or(0);
                        req.max(sal).min(100) as u8
                    })
                    .unwrap_or(0);
                if score > 0 {
                    row.insert(fac_id, score);
                }
            }
            if !row.is_empty() {
                table.insert(era_id, row);
            }
        }
        if !table.is_empty() {
            out.insert(name.to_string(), table);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eras_parse_sort_and_default_open_end() {
        // Deliberately out of `from` order. The second era has an explicit null `to` (an
        // open-ended current era), which is what the 9999 sentinel default is for.
        let json = r#"{"eras":[
            {"id":2,"name":"Star League","years":{"from":2571,"to":2780}},
            {"id":9,"name":"Dark Age","years":{"from":3132,"to":null}}
        ]}"#;
        let eras = parse_eras(json).unwrap();
        // Sorted ascending by `from`.
        assert_eq!(eras.iter().map(|e| e.id).collect::<Vec<_>>(), vec![2, 9]);
        assert_eq!(eras[0].name, "Star League");
        assert_eq!((eras[0].from, eras[0].to), (2571, 2780));
        // A null `to` defaults to the 9999 open-ended sentinel.
        assert_eq!(eras[1].to, 9999);
    }

    #[test]
    fn eras_missing_to_key_is_dropped() {
        // CAVEAT (current behavior, not necessarily intended): a *missing* `to` key short-circuits
        // the `?` in parse_eras and drops the whole era, whereas an explicit null `to` defaults to
        // 9999. If the upstream source ever omits `to` for the open era instead of nulling it, that
        // era silently vanishes. This test pins the behavior so a future fix has to update it.
        let json = r#"{"eras":[
            {"id":2,"name":"Star League","years":{"from":2571,"to":2780}},
            {"id":9,"name":"Dark Age","years":{"from":3132}}
        ]}"#;
        let eras = parse_eras(json).unwrap();
        assert_eq!(eras.iter().map(|e| e.id).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn eras_to_is_clamped_to_9999() {
        let json = r#"{"eras":[{"id":1,"name":"X","years":{"from":3000,"to":100000}}]}"#;
        assert_eq!(parse_eras(json).unwrap()[0].to, 9999);
    }

    #[test]
    fn eras_errors_are_distinct() {
        assert!(parse_eras("not json").is_err());
        // Valid JSON, but no `eras` array.
        assert!(parse_eras(r#"{"foo":1}"#).is_err());
        // Present but empty → an explicit "no eras parsed" error rather than an empty Vec.
        assert!(parse_eras(r#"{"eras":[]}"#).is_err());
    }

    #[test]
    fn factions_sort_by_group_then_name() {
        // Input order is intentionally scrambled across groups.
        let json = r#"{"factions":[
            {"id":3,"name":"Wolf","group":"Clan"},
            {"id":1,"name":"Steiner","group":"IS"},
            {"id":2,"name":"Jade Falcon","group":"Clan"},
            {"id":4,"name":"Davion"}
        ]}"#;
        let factions = parse_factions(json).unwrap();
        // Ordered by (group, name): "" (Davion) < "Clan" (Jade Falcon, Wolf) < "IS" (Steiner).
        let order: Vec<_> = factions.iter().map(|f| (f.group.as_str(), f.name.as_str())).collect();
        assert_eq!(
            order,
            vec![("", "Davion"), ("Clan", "Jade Falcon"), ("Clan", "Wolf"), ("IS", "Steiner")],
        );
        // A missing `group` becomes an empty string, not a parse failure.
        assert_eq!(factions[0].group, "");
    }

    #[test]
    fn factions_errors() {
        assert!(parse_factions("}{").is_err());
        assert!(parse_factions(r#"{"nope":[]}"#).is_err());
        assert!(parse_factions(r#"{"factions":[]}"#).is_err());
    }

    #[test]
    fn availability_takes_max_of_req_and_salvage_and_caps_at_100() {
        let known: HashSet<u16> = [10u16, 20].into_iter().collect();
        // Atlas: era 5 → faction 10 has salvage(7) > req(3) ⇒ 7; faction 20 caps 120→100.
        let json = r#"[
            {"n":"BMAtlas_AS7D","e":{"5":{"10":[3,7],"20":[120,0]}}}
        ]"#;
        let out = parse_availability(json, &known).unwrap();
        let atlas = &out["BMAtlas_AS7D"];
        assert_eq!(atlas[&5][&10], 7);
        assert_eq!(atlas[&5][&20], 100);
    }

    #[test]
    fn availability_drops_zeros_unknown_factions_and_empty_rows() {
        let known: HashSet<u16> = [10u16].into_iter().collect();
        let json = r#"[
            {"n":"Keep","e":{"5":{"10":[1,0]}}},
            {"n":"ZeroScore","e":{"5":{"10":[0,0]}}},
            {"n":"UnknownFac","e":{"5":{"99":[5,5]}}}
        ]"#;
        let out = parse_availability(json, &known).unwrap();
        // Nonzero, known faction → kept.
        assert_eq!(out["Keep"][&5][&10], 1);
        // A unit whose only score is zero is dropped entirely (no empty rows/tables).
        assert!(!out.contains_key("ZeroScore"));
        // A faction id absent from `known` is filtered, leaving the unit with no entries → dropped.
        assert!(!out.contains_key("UnknownFac"));
    }

    #[test]
    fn availability_requires_a_top_level_array() {
        let known: HashSet<u16> = HashSet::new();
        assert!(parse_availability(r#"{"not":"an array"}"#, &known).is_err());
    }
}
