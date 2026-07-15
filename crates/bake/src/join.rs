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

//! Join Mekbay's `units.json` + `equipment2.json` (+ per-unit SVG armor table) into our
//! immutable [`Mech`] spec.
//!
//! Almost everything we need is already resolved inside each unit's `comp[]` entries
//! (location `l`, damage `d`, range `r`, crit slots `c`, ammo tonnage `q` and total shots
//! `q2`). The *only* field that lives in `equipment2.json` is per-weapon **heat**, so we
//! build a small name/id/alias → heat index from it.

use neurohelmet_core::domain::{
    AmmoBin, ArcCard, ArcDamage, AsStats, CritSlot, Equipment, FiringArc, HeatSinkType, Location,
    LocationArmor, Mech, MechConfig, MotiveType, UnitType, WeaponMount,
};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// Parse the top-level `units.json` into the raw unit array.
pub fn parse_units(text: &str) -> Result<Vec<Value>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("units.json: {e}"))?;
    match v.get("units") {
        Some(Value::Array(a)) => Ok(a.clone()),
        _ => Err("units.json: missing `units` array".into()),
    }
}

pub fn is_mek(unit: &Value) -> bool {
    unit.get("type").and_then(Value::as_str) == Some("Mek")
}

/// The cache-relative SVG path for a unit, e.g. `sheets/mek/BMAtlas_AS7D.svg`.
pub fn sheet_rel(unit: &Value) -> Option<String> {
    let sheet = unit
        .get("sheets")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)?;
    Some(format!("sheets/{sheet}"))
}

/// Resolved stats for a weapon from `equipment2.json`.
#[derive(Clone)]
pub struct WeaponInfo {
    pub heat: u8,
    /// `"<ammoType>:<rackSize>"`, or `None` for energy weapons.
    pub ammo_key: Option<String>,
    /// Inherent to-hit modifier (`stats.toHitModifier`; pulse −2, heavy +1, …), 0 if absent.
    pub to_hit: i8,
    /// Eligible for a Targeting Computer's −1: `F_DIRECT_FIRE` and not `F_CWS`/`F_TASER`.
    pub tc_eligible: bool,
}

/// Which munition an ammo entry loads, and the `baseAmmo` group it belongs to.
#[derive(Clone)]
pub struct MunitionInfo {
    /// Canonical `baseAmmo` key (the standard entry's id) grouping a launcher's variants.
    pub base_ammo: String,
    /// Display name of this entry's munition (`mutatorName`, or `"Standard"` for the base).
    pub munition: String,
}

/// Lookups from `equipment2.json`, keyed by id / name / alias.
pub struct EqIndex {
    pub weapons: HashMap<String, WeaponInfo>,
    pub ammo: HashMap<String, String>,
    /// Munition + group for each ammo entry, by id / name / alias.
    pub munitions: HashMap<String, MunitionInfo>,
    /// `baseAmmo` group key -> selectable munition display names (`"Standard"` first), kept only
    /// for groups offering a real choice (more than one munition).
    pub munition_catalog: BTreeMap<String, Vec<String>>,
}

/// Every id/name/alias an entry can be referenced by.
fn entry_keys(key: &str, entry: &Value) -> Vec<String> {
    let mut keys = vec![key.to_string()];
    if let Some(id) = entry.get("id").and_then(Value::as_str) {
        keys.push(id.to_string());
    }
    if let Some(name) = entry.get("name").and_then(Value::as_str) {
        keys.push(name.to_string());
    }
    if let Some(aliases) = entry.get("aliases").and_then(Value::as_array) {
        keys.extend(aliases.iter().filter_map(Value::as_str).map(String::from));
    }
    keys
}

fn weapon_ammo_key(w: &Value) -> Option<String> {
    let at = w.get("ammoType").and_then(Value::as_str)?;
    if at.is_empty() || at == "NA" {
        return None;
    }
    let rs = w.get("rackSize").and_then(Value::as_u64)?;
    Some(format!("{at}:{rs}"))
}

fn ammo_ammo_key(a: &Value) -> Option<String> {
    let t = a.get("type").and_then(Value::as_str)?;
    let rs = a.get("rackSize").and_then(Value::as_u64)?;
    Some(format!("{t}:{rs}"))
}

/// Default munition display name when an ammo entry carries no `mutatorName` (the standard load).
const STANDARD: &str = "Standard";

/// Resolve an ammo entry's `(base_ammo group, munition display name)`. The base group is the
/// entry's `baseAmmo` when set, else its own `id` (a standard entry is the base of its group).
fn munition_info(key: &str, a: &Value) -> MunitionInfo {
    let base_ammo = a
        .get("baseAmmo")
        .and_then(Value::as_str)
        .unwrap_or(key)
        .to_string();
    let munition = a
        .get("mutatorName")
        .and_then(Value::as_str)
        .filter(|m| !m.is_empty())
        .unwrap_or(STANDARD)
        .to_string();
    MunitionInfo {
        base_ammo,
        munition,
    }
}

/// Build weapon (heat + ammo key) and ammo (compatibility key) lookups from `equipment2.json`.
pub fn build_equipment_index(eq_text: &str) -> Result<EqIndex, String> {
    let v: Value = serde_json::from_str(eq_text).map_err(|e| format!("equipment2.json: {e}"))?;
    let map = v
        .get("equipment")
        .and_then(Value::as_object)
        .ok_or("equipment2.json: missing `equipment` object")?;

    let mut weapons = HashMap::new();
    let mut ammo = HashMap::new();
    let mut munitions = HashMap::new();
    // base_ammo group -> set of munition display names (insertion-ordered via Vec + contains).
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (key, entry) in map {
        match entry.get("type").and_then(Value::as_str) {
            Some("weapon") => {
                let w = entry.get("weapon");
                // A weapon that omits `heat` generates zero heat (MGs, pods, Narc, ...) —
                // Mekbay leaves the field out rather than writing 0. Default it explicitly.
                let heat = w
                    .and_then(|w| w.get("heat"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(u8::MAX as u64) as u8;
                let ammo_key = w.and_then(weapon_ammo_key);
                // Inherent to-hit modifier lives under `stats`, not the `weapon` object.
                let to_hit = entry
                    .get("stats")
                    .and_then(|s| s.get("toHitModifier"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .clamp(i8::MIN as i64, i8::MAX as i64) as i8;
                // Targeting-Computer eligibility, per MegaMek: direct-fire, not CWS/Taser.
                let has_flag = |f: &str| {
                    entry
                        .get("flags")
                        .and_then(Value::as_array)
                        .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(f)))
                };
                let tc_eligible =
                    has_flag("F_DIRECT_FIRE") && !has_flag("F_CWS") && !has_flag("F_TASER");
                let info = WeaponInfo {
                    heat,
                    ammo_key,
                    to_hit,
                    tc_eligible,
                };
                for k in entry_keys(key, entry) {
                    weapons.insert(k, info.clone());
                }
            }
            Some("ammo") => {
                let Some(a) = entry.get("ammo") else { continue };
                if let Some(ak) = ammo_ammo_key(a) {
                    for k in entry_keys(key, entry) {
                        ammo.insert(k, ak.clone());
                    }
                }
                let info = munition_info(key, a);
                let names = groups.entry(info.base_ammo.clone()).or_default();
                if !names.contains(&info.munition) {
                    names.push(info.munition.clone());
                }
                for k in entry_keys(key, entry) {
                    munitions.insert(k, info.clone());
                }
            }
            _ => {}
        }
    }

    // Keep only groups that offer a real choice, with "Standard" first then the rest sorted.
    let munition_catalog = groups
        .into_iter()
        .filter(|(_, names)| names.len() > 1)
        .map(|(base, mut names)| {
            names.sort_by(|a, b| (a != STANDARD, a.as_str()).cmp(&(b != STANDARD, b.as_str())));
            (base, names)
        })
        .collect();

    Ok(EqIndex {
        weapons,
        ammo,
        munitions,
        munition_catalog,
    })
}

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn num_u16(v: &Value, key: &str) -> u16 {
    v.get(key)
        .and_then(Value::as_f64)
        .map(|f| f.round() as u16)
        .unwrap_or(0)
}

fn num_u32(v: &Value, key: &str) -> u32 {
    v.get(key)
        .and_then(Value::as_f64)
        .map(|f| f.round() as u32)
        .unwrap_or(0)
}

fn num_u64(v: &Value, key: &str) -> u64 {
    v.get(key)
        .and_then(Value::as_f64)
        .map(|f| f.round() as u64)
        .unwrap_or(0)
}

fn num_u8(v: &Value, key: &str) -> u8 {
    v.get(key)
        .and_then(Value::as_f64)
        .map(|f| f.round().clamp(0.0, 255.0) as u8)
        .unwrap_or(0)
}

/// Parse a JSON string array at `key` into owned strings, skipping any non-string entries; empty
/// when the key is absent or not an array. Shared by chassis quirks (Mekbay's `unit.quirks`, a flat
/// array of display-ready names like `["Command Mech", "Narrow/Low Profile"]`) and the AS-card /
/// firing-arc `specials`.
fn str_array(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Parse the Mekbay `as` block into Alpha Strike card stats (empty default if absent).
fn parse_as_stats(unit: &Value) -> AsStats {
    let Some(a) = unit.get("as") else {
        return AsStats::default();
    };
    let dmg = |k: &str| {
        a.get("dmg")
            .and_then(|d| d.get(k))
            .and_then(Value::as_str)
            .unwrap_or("0")
            .to_string()
    };
    let specials = str_array(a, "specials");
    AsStats {
        pv: num_u16(a, "PV"),
        size: num_u8(a, "SZ"),
        tp: s(a, "TP"),
        movement: s(a, "MV"),
        tmm: num_u8(a, "TMM"),
        armor: num_u8(a, "Arm"),
        structure: num_u8(a, "Str"),
        dmg_s: dmg("dmgS"),
        dmg_m: dmg("dmgM"),
        dmg_l: dmg("dmgL"),
        dmg_e: dmg("dmgE"),
        overheat: num_u8(a, "OV"),
        threshold: num_u8(a, "Th"),
        dt_rating: dt_rating(&specials),
        door_count: door_count(&specials),
        arcs: parse_arcs(a),
        specials,
    }
}

/// The large-craft DropShip-Transport (DT) rating from a `DT#` SUA (e.g. `DT4` → 4). Large craft
/// carry at most one; 0 if absent. Drives the "Dock" critical (IO:BF p.85).
fn dt_rating(specials: &[String]) -> u8 {
    specials
        .iter()
        .find_map(|t| t.strip_prefix("DT").and_then(|n| n.parse().ok()))
        .unwrap_or(0)
}

/// Total transport-bay doors: the sum of the `-D#` suffixes across a unit's transport SUAs
/// (e.g. `AT40-D6` contributes 6, `MT12-D4` contributes 4). Drives the "Door" critical
/// (IO:BF p.85). 0 when the unit carries no doored bays.
fn door_count(specials: &[String]) -> u16 {
    specials
        .iter()
        .filter_map(|t| t.rsplit_once("-D").and_then(|(_, n)| n.parse::<u16>().ok()))
        .sum()
}

/// Parse the large-craft multi-arc block (`frontArc/leftArc/rightArc/rearArc`) from an `as` block.
/// Returns `None` unless `usesArcs` is set — single-arc fighters/ground keep `None`.
fn parse_arcs(a: &Value) -> Option<ArcCard> {
    if a.get("usesArcs").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let arc = |k: &str| a.get(k).map(parse_firing_arc).unwrap_or_default();
    Some(ArcCard {
        front: arc("frontArc"),
        left: arc("leftArc"),
        right: arc("rightArc"),
        rear: arc("rearArc"),
    })
}

/// Parse one firing arc: the STD/CAP/SCAP/MSL damage sub-blocks (preserving `"0*"`) + arc specials.
fn parse_firing_arc(arc: &Value) -> FiringArc {
    let dmg = |class: &str| {
        let band = |b: &str| {
            arc.get(class)
                .and_then(|c| c.get(b))
                .and_then(Value::as_str)
                .unwrap_or("0")
                .to_string()
        };
        ArcDamage {
            s: band("dmgS"),
            m: band("dmgM"),
            l: band("dmgL"),
            e: band("dmgE"),
        }
    };
    let specials = str_array(arc, "specials");
    FiringArc {
        std: dmg("STD"),
        cap: dmg("CAP"),
        scap: dmg("SCAP"),
        msl: dmg("MSL"),
        specials,
    }
}

/// Result of building one mech, including any non-fatal warnings (e.g. weapons whose heat
/// could not be resolved).
pub struct BuildOutcome {
    pub mech: Mech,
    pub unresolved_heat: Vec<String>,
}

/// Whether a unit is a combat vehicle we track (tank / VTOL / naval — not aero/infantry).
pub fn is_vehicle(unit: &Value) -> bool {
    matches!(
        unit.get("type").and_then(Value::as_str),
        Some("Tank" | "VTOL" | "Naval")
    )
}

/// Whether a unit is infantry (conventional or Battle Armor). Phase 1 bakes them from JSON
/// only — their record sheets exist (BA sheets even carry per-trooper armor pips) but aren't
/// parsed yet, so they play on the Alpha Strike card.
pub fn is_infantry(unit: &Value) -> bool {
    unit.get("type").and_then(Value::as_str) == Some("Infantry")
}

/// Whether a unit is an aerospace / conventional FIGHTER (the §21 phase-1 slice). Big iron
/// (DropShips/WarShips/JumpShips/Small Craft/Space Stations) shares `type == "Aero"` but is
/// deferred, so we gate on the fighter subtypes. Baked from JSON only — AS card, no Classic doll.
pub fn is_aero_fighter(unit: &Value) -> bool {
    unit.get("type").and_then(Value::as_str) == Some("Aero")
        && matches!(
            unit.get("subtype").and_then(Value::as_str),
            Some(
                "Aerospace Fighter"
                    | "Aerospace Fighter Omni"
                    | "Conventional Fighter"
                    | "Fixed Wing Support Vehicle"
                    | "Fixed Wing Support Vehicle Omni"
            )
        )
}

/// Whether a unit is capital-scale large craft that fields on the AS/BF card. Phase 1 admitted the
/// **DropShip + Small Craft** subtypes (spheroid/aerodyne, civilian + military); Phase 2 adds
/// **JumpShip / WarShip / Space Station** (military + civilian). All are `type == "Aero"` and carry
/// the multi-arc card (`usesArcs`); baked from JSON only (no Classic capital record sheet). Every
/// subtype string here is verified against the source `units.json`.
pub fn is_large_craft(unit: &Value) -> bool {
    unit.get("type").and_then(Value::as_str) == Some("Aero")
        && matches!(
            unit.get("subtype").and_then(Value::as_str),
            Some(
                "Spheroid DropShip"
                    | "Aerodyne DropShip"
                    | "Civilian Spheroid DropShip"
                    | "Civilian Aerodyne DropShip"
                    | "Aerodyne Small Craft"
                    | "Spheroid Small Craft"
                    | "Civilian Aerodyne Small Craft"
                    | "JumpShip"
                    | "WarShip"
                    | "Military Space Station"
                    | "Civilian Space Station"
            )
        )
}

/// Parse a unit's `comp[]` into weapons / ammo / equipment (+ unresolved-heat warnings). Shared by
/// 'Mechs and vehicles — the mount types (E/M/B/A weapons, P physicals, X ammo, C/O gear) are the
/// same; only the surrounding chassis (armor, heat sinks, config) differs by unit kind.
/// Resolve a comp entry's `l` location code to a [`Location`]. Tries the canonical Mekbay code
/// (`HD`, `RT`, `T3`, …, splitting a two-location mount like artillery `"RT/RA"` on the first),
/// then an infantry `"Trooper N"` label, then the caller's `fallback`. Infantry/BA weapons carry
/// squad-wide labels (`Troop`/`Squad`/`Point`/`FGUN`) that are not real location codes, so without
/// a fallback they would be dropped — which is why every infantry/BA weapon used to vanish.
fn comp_location(raw: Option<&str>, fallback: Option<Location>) -> Option<Location> {
    raw.and_then(|l| Location::from_code(l.split('/').next().unwrap_or(l)))
        .or_else(|| raw.and_then(trooper_label))
        .or(fallback)
}

/// Map a Battle Armor `"Trooper N"` label (1..=6) to its trooper armor track.
fn trooper_label(l: &str) -> Option<Location> {
    let n: usize = l.strip_prefix("Trooper ")?.trim().parse().ok()?;
    Location::TROOPERS.get(n.checked_sub(1)?).copied()
}

/// Parse a unit's `comp[]` into weapons / ammo / equipment. `fallback_loc` is `Some` only for
/// infantry/BA: it supplies the location for squad-wide weapon labels, and signals that `q` is the
/// trooper count (so weapons are listed once, not expanded into N mounts) and that the heatless
/// small arms are expected (not flagged as "unresolved heat").
fn parse_loadout(
    comp: &[Value],
    idx: &EqIndex,
    fallback_loc: Option<Location>,
) -> (Vec<WeaponMount>, Vec<AmmoBin>, Vec<Equipment>, Vec<String>) {
    let infantry = fallback_loc.is_some();
    let mut weapons = Vec::new();
    let mut ammo = Vec::new();
    let mut equipment = Vec::new();
    let mut next_weapon_id = 0u32;
    let mut next_ammo_id = 0u32;
    let mut unresolved_heat = Vec::new();

    for c in comp {
        let t = s(c, "t");
        let Some(location) = comp_location(c.get("l").and_then(Value::as_str), fallback_loc) else {
            continue; // no real location and no fallback (e.g. mech armor-type rows with p=-1)
        };
        let name = s(c, "n");
        // Mechs/vehicles: `q` is the mount count (2 Medium Lasers → 2 rows). Infantry: `q` is the
        // trooper count (12 auto-rifles is one weapon type, not 12 rows), so list it once.
        let qty = if infantry { 1 } else { num_u16(c, "q").max(1) };

        match t.as_str() {
            // Ammunition: q = tons, q2 = total shots in the bin.
            "X" => {
                let total_shots = num_u16(c, "q2");
                let tons = num_u16(c, "q").max(1);
                let (tons, shots_per_ton) = if total_shots.is_multiple_of(tons) {
                    (tons, total_shots / tons)
                } else {
                    (1, total_shots)
                };
                let id = s(c, "id");
                let ammo_key = idx.ammo.get(&id).or_else(|| idx.ammo.get(&name)).cloned();
                let minfo = idx.munitions.get(&id).or_else(|| idx.munitions.get(&name));
                let base_ammo = minfo
                    .map(|mi| mi.base_ammo.clone())
                    .filter(|b| idx.munition_catalog.contains_key(b));
                let munition = minfo
                    .map(|mi| mi.munition.clone())
                    .filter(|m| m != STANDARD)
                    .unwrap_or_default();
                ammo.push(AmmoBin {
                    id: next_ammo_id,
                    name,
                    location,
                    shots_per_ton,
                    tons,
                    ammo_key,
                    munition,
                    base_ammo,
                });
                next_ammo_id += 1;
            }
            // Weapons: energy / missile / ballistic / artillery. Expand `q` copies.
            "E" | "M" | "B" | "A" => {
                let id = s(c, "id");
                let info = idx.weapons.get(&id).or_else(|| idx.weapons.get(&name));
                // Infantry small arms are intentionally heatless (and not in the weapon index),
                // so don't count them as genuinely-missing heat data.
                if info.is_none() && !infantry {
                    unresolved_heat.push(name.clone());
                }
                let heat = info.map(|i| i.heat).unwrap_or(0);
                let ammo_key = info.and_then(|i| i.ammo_key.clone());
                let to_hit = info.map(|i| i.to_hit).unwrap_or(0);
                let tc_eligible = info.is_some_and(|i| i.tc_eligible);
                let rear = c.get("rear").and_then(Value::as_bool).unwrap_or(false);
                let damage = s(c, "d");
                let range = s(c, "r");
                let crit_slots = num_u8(c, "c");
                // Infantry: `q` is the trooper count carrying this weapon (group damage = count × d,
                // since `d` is per trooper). Mechs/vehicles list identical mounts separately, so 1.
                let count = if infantry { num_u16(c, "q").max(1) } else { 1 };
                for _ in 0..qty {
                    weapons.push(WeaponMount {
                        id: next_weapon_id,
                        name: name.clone(),
                        location,
                        rear,
                        heat,
                        damage: damage.clone(),
                        range: range.clone(),
                        crit_slots,
                        ammo_key: ammo_key.clone(),
                        to_hit,
                        tc_eligible,
                        count,
                    });
                    next_weapon_id += 1;
                }
            }
            // Physical weapons (Hatchet, Sword, Claw, ...): melee, no heat/ammo/range.
            "P" => {
                let damage = s(c, "d");
                let crit_slots = num_u8(c, "c");
                for _ in 0..qty {
                    weapons.push(WeaponMount {
                        id: next_weapon_id,
                        name: name.clone(),
                        location,
                        rear: false,
                        heat: 0,
                        damage: damage.clone(),
                        range: String::new(),
                        crit_slots,
                        ammo_key: None,
                        to_hit: 0,
                        tc_eligible: false,
                        count: 1,
                    });
                    next_weapon_id += 1;
                }
            }
            // Mounted gear: jump jets, CASE, ECM, TAG, C3, targeting computer, MASC, ...
            "C" | "O" => {
                if !name.contains("Heat Sink") {
                    equipment.push(Equipment { name, location });
                }
            }
            _ => {} // structure, actuators, armor-type rows — not tracked individually
        }
    }
    (weapons, ammo, equipment, unresolved_heat)
}

/// Build an infantry unit (conventional platoon or Battle Armor squad). Battle Armor passes
/// the per-trooper armor table parsed from its sheet (`svg::parse_ba_armor`); conventional
/// infantry has no pips, so a single [`Location::Platoon`] strength track is synthesized from
/// the catalog's `internal` (troop count). The `as` block stays the Alpha Strike surface; the
/// shared loadout parse fills weapons/gear for the picker preview.
pub fn build_infantry(
    unit: &Value,
    mut armor: BTreeMap<Location, LocationArmor>,
    idx: &EqIndex,
) -> Result<BuildOutcome, String> {
    let chassis = s(unit, "chassis");
    if chassis.is_empty() {
        return Err("unit has no chassis".into());
    }
    let comp = unit
        .get("comp")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let unit_type = if s(unit, "subtype") == "Battle Armor" {
        UnitType::BattleArmor
    } else {
        UnitType::Infantry
    };
    // Infantry weapons carry squad-wide location labels with no real code; fall them back to the
    // unit's single playable track (BA's first trooper / the conventional platoon) so they aren't
    // dropped. Explicit `Trooper N` labels still map to their own track inside `comp_location`.
    let fallback = Some(match unit_type {
        UnitType::BattleArmor => Location::Trooper1,
        _ => Location::Platoon,
    });
    let (weapons, ammo, equipment, unresolved_heat) = parse_loadout(&comp, idx, fallback);
    if unit_type == UnitType::Infantry {
        armor = BTreeMap::from([(
            Location::Platoon,
            LocationArmor {
                armor_max: 0,
                rear_max: 0,
                internal_max: num_u16(unit, "internal"),
            },
        )]);
    }
    let mech = Mech {
        chassis,
        model: s(unit, "model"),
        tonnage: num_u16(unit, "tons"),
        tech_base: s(unit, "techBase"),
        role: s(unit, "role"),
        weight_class: s(unit, "weightClass"),
        subtype: s(unit, "subtype"),
        year: num_u16(unit, "year"),
        bv: num_u32(unit, "bv"),
        cost: num_u64(unit, "cost"),
        armor_type: s(unit, "armorType"),
        structure_type: String::new(),
        walk: num_u8(unit, "walk"),
        run: num_u8(unit, "run"),
        jump: num_u8(unit, "jump"),
        heat_sinks: 0,
        heat_sink_type: HeatSinkType::Single,
        dissipation: 0,
        config: MechConfig::Biped,
        unit_type,
        motive: None,
        internal: num_u16(unit, "internal"),
        dpt: num_u16(unit, "dpt"),
        transport: Vec::new(),
        armor,
        weapons,
        ammo,
        equipment,
        crit_slots: BTreeMap::new(),
        as_stats: parse_as_stats(unit),
        availability: BTreeMap::new(),
        quirks: str_array(unit, "quirks"),
    };
    Ok(BuildOutcome {
        mech,
        unresolved_heat,
    })
}

/// Build an aerospace / conventional fighter (§21). `armor` is parsed from the record sheet — four
/// armor arcs (NOS/LWG/RWG/AFT) plus the shared Structural Integrity pool (`AeroSI`, from the SI
/// structure pips) — and `heat_sinks` from the sheet (`svg::parse_aero_heat_sinks`). Weapons keep
/// their arc locations now that `Location` knows the codes. Movement is thrust (`walk`/`run` =
/// Safe/Max). No crit-slot table (the aero CRITICAL DAMAGE track is a manual rolled list).
pub fn build_aero(
    unit: &Value,
    armor: BTreeMap<Location, LocationArmor>,
    heat_sinks: u16,
    dissipation: u16,
    idx: &EqIndex,
) -> Result<BuildOutcome, String> {
    let chassis = s(unit, "chassis");
    if chassis.is_empty() {
        return Err("unit has no chassis".into());
    }
    let comp = unit
        .get("comp")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let (weapons, ammo, equipment, unresolved_heat) = parse_loadout(&comp, idx, None);
    let mech = Mech {
        chassis,
        model: s(unit, "model"),
        tonnage: num_u16(unit, "tons"),
        tech_base: s(unit, "techBase"),
        role: s(unit, "role"),
        weight_class: s(unit, "weightClass"),
        subtype: s(unit, "subtype"),
        year: num_u16(unit, "year"),
        bv: num_u32(unit, "bv"),
        cost: num_u64(unit, "cost"),
        armor_type: s(unit, "armorType"),
        structure_type: String::new(),
        walk: num_u8(unit, "walk"), // Safe Thrust
        run: num_u8(unit, "run"),   // Maximum Thrust
        jump: 0,
        heat_sinks,
        // Doubles dissipate more than their count (e.g. 16 sinks → 32), shown as "N× Double".
        heat_sink_type: if dissipation > heat_sinks {
            HeatSinkType::Double
        } else {
            HeatSinkType::Single
        },
        dissipation,
        config: MechConfig::Biped,
        unit_type: UnitType::Aerospace,
        motive: None,
        internal: 0, // SI lives in the AeroSI location (parsed from the sheet's SI pips)
        dpt: 0,
        transport: Vec::new(),
        armor, // 4 arcs (NOS/LWG/RWG/AFT) + AeroSI (SI structure pips), from the record sheet
        weapons,
        ammo,
        equipment,
        crit_slots: BTreeMap::new(),
        as_stats: parse_as_stats(unit),
        availability: BTreeMap::new(),
        quirks: str_array(unit, "quirks"),
    };
    Ok(BuildOutcome {
        mech,
        unresolved_heat,
    })
}

/// Build a large craft (DropShip / Small Craft; Phase 2 adds JumpShip / WarShip / Space Station).
/// These field on the AS/BF card only — the full multi-arc damage matrix + single Arm/Str/Th pool
/// live in the JSON `as` block ([`parse_as_stats`] carries the arcs). The Classic capital record
/// sheet (per-arc armor doll, weapon bays) is out of scope, so there is no SVG-derived armor doll,
/// no heat-sink model, and no Classic weapon loadout (the arcs ARE the weapon representation) —
/// baked from JSON only, which also keeps large WarShip loadouts from bloating the bundle.
pub fn build_large_craft(unit: &Value) -> Result<BuildOutcome, String> {
    let chassis = s(unit, "chassis");
    if chassis.is_empty() {
        return Err("unit has no chassis".into());
    }
    let mech = Mech {
        chassis,
        model: s(unit, "model"),
        tonnage: num_u16(unit, "tons"),
        tech_base: s(unit, "techBase"),
        role: s(unit, "role"),
        weight_class: s(unit, "weightClass"),
        subtype: s(unit, "subtype"),
        year: num_u16(unit, "year"),
        bv: num_u32(unit, "bv"),
        cost: num_u64(unit, "cost"),
        armor_type: s(unit, "armorType"),
        structure_type: String::new(),
        walk: num_u8(unit, "walk"), // Safe Thrust
        run: num_u8(unit, "run"),   // Maximum Thrust
        jump: 0,
        heat_sinks: 0,
        heat_sink_type: HeatSinkType::Single,
        dissipation: 0,
        config: MechConfig::Biped,
        unit_type: UnitType::Aerospace,
        motive: None,
        internal: 0, // SI is carried as the AS `Str` pool on `as_stats`, not a Classic doll
        dpt: 0,
        transport: Vec::new(),
        armor: BTreeMap::new(), // AS/BF card only; the single Arm/Str/Th pool lives on `as_stats`
        weapons: Vec::new(),    // the firing arcs are the weapon representation
        ammo: Vec::new(),
        equipment: Vec::new(),
        crit_slots: BTreeMap::new(),
        as_stats: parse_as_stats(unit), // includes the multi-arc card (`usesArcs`)
        availability: BTreeMap::new(),
        quirks: str_array(unit, "quirks"),
    };
    Ok(BuildOutcome {
        mech,
        unresolved_heat: Vec::new(),
    })
}

/// Build a combat vehicle. Reuses the shared loadout parse and the mech armor model: vehicle record
/// sheets carry per-location armor + internal pips just like 'Mechs (`parse_armor` handles them once
/// `Location` knows the FR/LS/RS/RR/TU codes). Vehicles have no heat sinks and no crit-slot table
/// (`crit_slots` stays empty — vehicle criticals are a manual rolled table, not slots).
pub fn build_vehicle(
    unit: &Value,
    armor: BTreeMap<Location, LocationArmor>,
    transport: Vec<String>,
    idx: &EqIndex,
) -> Result<BuildOutcome, String> {
    let chassis = s(unit, "chassis");
    if chassis.is_empty() {
        return Err("vehicle has no chassis".into());
    }
    let comp = unit
        .get("comp")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let (weapons, ammo, equipment, unresolved_heat) = parse_loadout(&comp, idx, None);
    let mech = Mech {
        chassis,
        model: s(unit, "model"),
        tonnage: num_u16(unit, "tons"),
        tech_base: s(unit, "techBase"),
        role: s(unit, "role"),
        weight_class: s(unit, "weightClass"),
        subtype: s(unit, "subtype"),
        year: num_u16(unit, "year"),
        bv: num_u32(unit, "bv"),
        cost: num_u64(unit, "cost"),
        armor_type: s(unit, "armorType"),
        structure_type: s(unit, "structureType"),
        walk: num_u8(unit, "walk"),
        run: num_u8(unit, "run"),
        jump: num_u8(unit, "jump"),
        heat_sinks: 0,
        heat_sink_type: HeatSinkType::Single,
        dissipation: 0,
        config: MechConfig::Biped,
        unit_type: UnitType::Vehicle,
        motive: MotiveType::from_move_type(&s(unit, "moveType")),
        internal: num_u16(unit, "internal"),
        dpt: 0,
        transport,
        armor,
        weapons,
        ammo,
        equipment,
        crit_slots: BTreeMap::new(),
        as_stats: parse_as_stats(unit),
        availability: BTreeMap::new(),
        quirks: str_array(unit, "quirks"),
    };
    Ok(BuildOutcome {
        mech,
        unresolved_heat,
    })
}

/// Build a [`Mech`] from a unit value, its parsed armor table, and the equipment index.
pub fn build_mech(
    unit: &Value,
    armor: BTreeMap<Location, LocationArmor>,
    mut crit_slots: BTreeMap<Location, Vec<CritSlot>>,
    idx: &EqIndex,
) -> Result<BuildOutcome, String> {
    let chassis = s(unit, "chassis");
    if chassis.is_empty() {
        return Err("unit has no chassis".into());
    }

    let comp = unit
        .get("comp")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // Crit slots carry the equipment *id* as their name (e.g. "CLSRM6", "Autocannon/20",
    // "Clan Ammo SRM-6"); translate those to the friendly display name from the same comp entry
    // ("SRM 6", "AC/20", "SRM 6 Ammo") so the crit popup is readable and weapon<->slot name
    // matching works. Systems (engine/gyro/cockpit/...) aren't in comp and keep their names.
    let id_to_name: HashMap<&str, &str> = comp
        .iter()
        .filter_map(|c| {
            let id = c.get("id").and_then(Value::as_str)?;
            let n = c.get("n").and_then(Value::as_str)?;
            (!id.is_empty() && !n.is_empty()).then_some((id, n))
        })
        .collect();
    for slots in crit_slots.values_mut() {
        for cs in slots.iter_mut().filter(|c| !c.system) {
            if let Some(name) = id_to_name.get(cs.name.as_str()) {
                cs.name = (*name).to_string();
            }
        }
    }

    // Chassis configuration (`moveType`: Biped / Quad / Tripod) — drives the hit-location set.
    let config = match s(unit, "moveType").as_str() {
        "Quad" => MechConfig::Quad,
        "Tripod" => MechConfig::Tripod,
        _ => MechConfig::Biped,
    };

    // Descriptive metadata for the picker preview.
    let year = num_u16(unit, "year");
    let armor_type = s(unit, "armorType");
    let structure_type = s(unit, "structureType");

    let as_stats = parse_as_stats(unit);

    let dissipation = num_u16(unit, "dissipation");

    // Heat sink type: a single "Double"/"Clan Double" sink in the component list flips it.
    let has_double = comp
        .iter()
        .filter(|c| c.get("t").and_then(Value::as_str) == Some("C"))
        .any(|c| s(c, "n").contains("Double"));
    let heat_sink_type = if has_double {
        HeatSinkType::Double
    } else {
        HeatSinkType::Single
    };
    let per_sink = heat_sink_type.per_sink();
    let heat_sinks = if per_sink > 0 {
        dissipation / per_sink
    } else {
        0
    };

    let (weapons, ammo, equipment, unresolved_heat) = parse_loadout(&comp, idx, None);

    let mech = Mech {
        chassis,
        model: s(unit, "model"),
        tonnage: num_u16(unit, "tons"),
        tech_base: s(unit, "techBase"),
        role: s(unit, "role"),
        weight_class: s(unit, "weightClass"),
        subtype: s(unit, "subtype"),
        year,
        bv: num_u32(unit, "bv"),
        cost: num_u64(unit, "cost"),
        armor_type,
        structure_type,
        walk: num_u8(unit, "walk"),
        run: num_u8(unit, "run"),
        jump: num_u8(unit, "jump"),
        heat_sinks,
        heat_sink_type,
        dissipation,
        config,
        unit_type: UnitType::Mech,
        motive: None,
        internal: 0,
        dpt: 0,
        transport: Vec::new(),
        armor,
        weapons,
        ammo,
        equipment,
        crit_slots,
        as_stats,
        availability: BTreeMap::new(),
        quirks: str_array(unit, "quirks"),
    };
    Ok(BuildOutcome {
        mech,
        unresolved_heat,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn equipment_crit_names_translate_to_display_names() {
        // Clan SRM 6: comp id "CLSRM6" but display name "SRM 6"; the crit slot carries the id.
        let unit = json!({
            "chassis": "Ryoken III",
            "model": "Prime",
            "comp": [
                {"t": "M", "n": "SRM 6", "id": "CLSRM6", "l": "RT", "d": "2/msl", "r": "3/6/9", "c": "1"},
            ],
        });
        let mut crit_slots = BTreeMap::new();
        crit_slots.insert(
            Location::RightTorso,
            vec![
                CritSlot {
                    slot: 2,
                    name: "CLSRM6".into(),
                    system: false,
                    hittable: true,
                    ..Default::default()
                },
                CritSlot {
                    slot: 0,
                    name: "Fusion Engine".into(),
                    system: true,
                    hittable: true,
                    ..Default::default()
                },
            ],
        );
        let idx = EqIndex {
            weapons: HashMap::new(),
            ammo: HashMap::new(),
            munitions: HashMap::new(),
            munition_catalog: BTreeMap::new(),
        };
        let out = build_mech(&unit, BTreeMap::new(), crit_slots, &idx).unwrap();
        let rt = &out.mech.crit_slots[&Location::RightTorso];

        // The equipment slot is translated to the friendly name...
        assert_eq!(rt.iter().find(|c| c.slot == 2).unwrap().name, "SRM 6");
        // ...so it now equals the weapon name (which is how disable-on-crit links them).
        assert_eq!(out.mech.weapons[0].name, "SRM 6");
        // System slots are left untouched (not in comp).
        assert_eq!(
            rt.iter().find(|c| c.slot == 0).unwrap().name,
            "Fusion Engine"
        );
    }

    #[test]
    fn str_array_parses_string_arrays() {
        // Present: a flat array of display-ready strings, kept verbatim in order (e.g. quirks).
        let unit = json!({ "quirks": ["Command Mech", "Narrow/Low Profile"] });
        assert_eq!(
            str_array(&unit, "quirks"),
            vec!["Command Mech", "Narrow/Low Profile"]
        );
        // Empty array and absent key both yield nothing.
        assert!(str_array(&json!({ "quirks": [] }), "quirks").is_empty());
        assert!(str_array(&json!({ "chassis": "Locust" }), "quirks").is_empty());
        // Non-string entries are skipped rather than panicking.
        assert_eq!(
            str_array(&json!({ "quirks": ["Stable", 7, null] }), "quirks"),
            vec!["Stable"]
        );
    }

    #[test]
    fn infantry_weapons_survive_squad_wide_location_labels() {
        // AA Mechanized Infantry shape: weapons carry `l:"Troop"` (no real location code) and
        // `q` is the trooper count — they must not be dropped, nor expanded into N rows.
        let unit = json!({
            "chassis": "AA Mechanized Infantry",
            "model": "Mechanized AA Infantry",
            "subtype": "Mechanized Conventional Infantry",
            "internal": 12,
            "comp": [
                {"t": "B", "n": "Auto-Rifle (Modern, Generic)", "id": "InfantryAssaultRifle", "l": "Troop", "d": "0.52", "r": "1", "q": 12},
                {"t": "M", "n": "AA Weapon (Mk. 2, Man-Portable)", "id": "AA Weapon (Mk. 2, Man-Portable)", "l": "Troop", "d": "0.81", "r": "2", "q": 8},
                {"t": "S", "n": "Clan Armor Kit (All)", "l": "TPRS", "q": 1},
            ],
        });
        let idx = EqIndex {
            weapons: HashMap::new(),
            ammo: HashMap::new(),
            munitions: HashMap::new(),
            munition_catalog: BTreeMap::new(),
        };
        let out = build_infantry(&unit, BTreeMap::new(), &idx).unwrap();
        // Both weapons survive, once each (q is troopers, not mounts); the armor-kit `S` row is skipped.
        assert_eq!(
            out.mech.weapons.len(),
            2,
            "both infantry weapons baked, not expanded"
        );
        assert_eq!(out.mech.weapons[0].name, "Auto-Rifle (Modern, Generic)");
        assert_eq!(out.mech.weapons[1].name, "AA Weapon (Mk. 2, Man-Portable)");
        // Squad-wide labels fall back to the platoon track; heatless arms aren't "unresolved".
        assert!(out
            .mech
            .weapons
            .iter()
            .all(|w| w.location == Location::Platoon));
        assert!(out.unresolved_heat.is_empty());
    }

    #[test]
    fn battle_armor_trooper_labels_map_to_their_track() {
        // BA weapons usually carry `l:"Squad"` (→ first trooper), but an explicit `Trooper N`
        // label maps to that suit's track.
        let unit = json!({
            "chassis": "Achileus Light Battle Armor",
            "model": "[David]",
            "subtype": "Battle Armor",
            "internal": 4,
            "comp": [
                {"t": "B", "n": "Gauss Rifle [David]", "l": "Squad", "d": "1", "r": "3/6/9", "q": 1},
                {"t": "B", "n": "Support Laser", "l": "Trooper 3", "d": "1", "r": "1/2/3", "q": 1},
            ],
        });
        let idx = EqIndex {
            weapons: HashMap::new(),
            ammo: HashMap::new(),
            munitions: HashMap::new(),
            munition_catalog: BTreeMap::new(),
        };
        let out = build_infantry(&unit, BTreeMap::new(), &idx).unwrap();
        assert_eq!(out.mech.unit_type, UnitType::BattleArmor);
        assert_eq!(out.mech.weapons[0].location, Location::Trooper1); // squad-wide fallback
        assert_eq!(out.mech.weapons[1].location, Location::Trooper3); // explicit label
    }

    #[test]
    fn parses_alpha_strike_block() {
        let unit = json!({
            "chassis": "Atlas",
            "model": "AS7-D",
            "as": {
                "PV": 52, "SZ": 4, "TP": "BM", "MV": "6\"", "TMM": 1,
                "Arm": 10, "Str": 8, "OV": 0,
                "dmg": {"dmgS": "5", "dmgM": "5", "dmgL": "2", "dmgE": "0"},
                "specials": ["AC2/2/-", "IF1"]
            }
        });
        let idx = EqIndex {
            weapons: HashMap::new(),
            ammo: HashMap::new(),
            munitions: HashMap::new(),
            munition_catalog: BTreeMap::new(),
        };
        let out = build_mech(&unit, BTreeMap::new(), BTreeMap::new(), &idx).unwrap();
        let a = &out.mech.as_stats;
        assert_eq!(a.pv, 52);
        assert_eq!(a.size, 4);
        assert_eq!(a.tp, "BM");
        assert_eq!(a.armor, 10);
        assert_eq!(a.structure, 8);
        assert_eq!(
            (a.dmg_s.as_str(), a.dmg_m.as_str(), a.dmg_l.as_str()),
            ("5", "5", "2")
        );
        assert_eq!(a.specials, vec!["AC2/2/-", "IF1"]);
    }

    #[test]
    fn builds_classic_aerospace_fighter() {
        // Visigoth-shaped: type Aero, AS block, thrust (walk/run), weapons in aero arcs (NOS/RWG).
        let unit = json!({
            "chassis": "Visigoth",
            "model": "Prime",
            "type": "Aero",
            "subtype": "Aerospace Fighter Omni",
            "techBase": "Clan",
            "weightClass": "Medium",
            "tons": 80,
            "walk": 5,
            "run": 8,
            "comp": [
                {"t": "E", "n": "ER Large Laser", "l": "NOS", "d": "8", "r": "5/10/15"},
                {"t": "M", "n": "LRM 10", "l": "RWG", "d": "6", "r": "7/14/21"},
            ],
            "as": {
                "TP": "AF", "SZ": 2, "PV": 50, "MV": "7a", "Arm": 7, "Str": 4, "OV": 0,
                "Th": 3, "usesTh": true,
                "dmg": {"dmgS": "6", "dmgM": "6", "dmgL": "5", "dmgE": "0"},
                "specials": ["BOMB2", "FUEL20", "VSTOL"]
            }
        });
        assert!(is_aero_fighter(&unit));
        // Armor as the record sheet would parse it: 4 arcs + the shared SI pool.
        let armor = BTreeMap::from([
            (
                Location::Nose,
                LocationArmor {
                    armor_max: 12,
                    rear_max: 0,
                    internal_max: 0,
                },
            ),
            (
                Location::LeftWing,
                LocationArmor {
                    armor_max: 9,
                    rear_max: 0,
                    internal_max: 0,
                },
            ),
            (
                Location::RightWing,
                LocationArmor {
                    armor_max: 9,
                    rear_max: 0,
                    internal_max: 0,
                },
            ),
            (
                Location::Aft,
                LocationArmor {
                    armor_max: 7,
                    rear_max: 0,
                    internal_max: 0,
                },
            ),
            (
                Location::AeroSI,
                LocationArmor {
                    armor_max: 0,
                    rear_max: 0,
                    internal_max: 5,
                },
            ),
        ]);
        let idx = EqIndex {
            weapons: HashMap::new(),
            ammo: HashMap::new(),
            munitions: HashMap::new(),
            munition_catalog: BTreeMap::new(),
        };
        let out = build_aero(&unit, armor, 16, 32, &idx).unwrap();
        let m = out.mech;
        assert_eq!(m.unit_type, UnitType::Aerospace);
        assert_eq!(m.subtype, "Aerospace Fighter Omni");
        assert_eq!((m.as_stats.tp.as_str(), m.as_stats.pv), ("AF", 50));
        assert_eq!(
            m.as_stats.threshold, 3,
            "aerospace Threshold baked from the `Th` field"
        );
        assert_eq!((m.walk, m.run), (5, 8), "safe/max thrust");
        // Doubles: 16 sinks dissipating 32.
        assert_eq!((m.heat_sinks, m.dissipation), (16, 32));
        assert_eq!(m.heat_sink_type, HeatSinkType::Double);
        // The 4 arcs + SI are on the doll; SI is the structure pool.
        assert_eq!(m.armor[&Location::AeroSI].internal_max, 5);
        assert!(m.armor.contains_key(&Location::Nose));
        // Weapons now KEEP their arc locations (no longer dropped).
        assert_eq!(m.weapons.len(), 2);
        assert_eq!(m.weapons[0].location, Location::Nose);
    }

    #[test]
    fn large_craft_bakes_multi_arc_card_from_json() {
        let unit = json!({
            "type": "Aero",
            "subtype": "Spheroid DropShip",
            "chassis": "Union",
            "model": "(2708)",
            "tons": 3600,
            "walk": 3,
            "run": 5,
            "as": {
                "TP": "DS", "SZ": 2, "MV": "3p", "Arm": 10, "Str": 5, "Th": 1, "PV": 200,
                "usesArcs": true, "usesTh": true,
                "dmg": {"dmgS": "0", "dmgM": "0", "dmgL": "0", "dmgE": "0"},
                "frontArc": {
                    "STD":  {"dmgS": "4", "dmgM": "3", "dmgL": "2", "dmgE": "0*"},
                    "CAP":  {"dmgS": "0", "dmgM": "0", "dmgL": "0", "dmgE": "0"},
                    "SCAP": {"dmgS": "0", "dmgM": "0", "dmgL": "0", "dmgE": "0"},
                    "MSL":  {"dmgS": "1", "dmgM": "1", "dmgL": "1", "dmgE": "1"},
                    "specials": ["PNT1"]
                },
                "leftArc":  {"STD": {"dmgS": "2", "dmgM": "2", "dmgL": "1", "dmgE": "0"}},
                "rightArc": {"STD": {"dmgS": "2", "dmgM": "2", "dmgL": "1", "dmgE": "0"}},
                "rearArc":  {"STD": {"dmgS": "1", "dmgM": "0", "dmgL": "0", "dmgE": "0"}},
                "specials": ["AT2-D2", "SPC", "CRW3"]
            }
        });
        assert!(is_large_craft(&unit));
        assert!(!is_aero_fighter(&unit));

        let m = build_large_craft(&unit).unwrap().mech;
        assert_eq!(m.unit_type, UnitType::Aerospace);
        assert_eq!(m.subtype, "Spheroid DropShip");
        // Single Arm/Str/Th pool + PV on the AS card; JSON-only (no doll, no loadout).
        assert_eq!(
            (m.as_stats.armor, m.as_stats.structure, m.as_stats.threshold),
            (10, 5, 1)
        );
        assert_eq!(m.as_stats.pv, 200);
        assert!(m.armor.is_empty() && m.weapons.is_empty());
        assert_eq!((m.walk, m.run), (3, 5), "safe/max thrust");

        let arcs = m
            .as_stats
            .arcs
            .expect("large craft carries the multi-arc card");
        assert_eq!(
            (
                arcs.front.std.s.as_str(),
                arcs.front.std.m.as_str(),
                arcs.front.std.l.as_str()
            ),
            ("4", "3", "2")
        );
        assert_eq!(
            arcs.front.std.e, "0*",
            "minimal-damage token preserved, not collapsed to 0"
        );
        assert_eq!(arcs.front.msl.l, "1");
        assert_eq!(arcs.front.specials, vec!["PNT1"]);
        assert_eq!(arcs.rear.std.s, "1");
        assert_eq!(arcs.left.cap.s, "0", "absent classes bake all-zero");

        // A fighter (usesArcs = false) keeps arcs = None.
        let fighter = json!({
            "type": "Aero", "subtype": "Aerospace Fighter", "chassis": "X", "model": "Y",
            "as": {"TP": "AF", "usesArcs": false, "dmg": {"dmgS": "3", "dmgM": "3", "dmgL": "2", "dmgE": "1"}}
        });
        assert!(!is_large_craft(&fighter));
        assert!(parse_as_stats(&fighter).arcs.is_none());
    }

    #[test]
    fn bakes_weight_class_and_subtype_for_filtering() {
        let unit = json!({
            "chassis": "Timber Wolf",
            "model": "Prime",
            "techBase": "Clan",
            "weightClass": "Heavy",
            "subtype": "BattleMek Omni",
            "tons": 75,
        });
        let idx = EqIndex {
            weapons: HashMap::new(),
            ammo: HashMap::new(),
            munitions: HashMap::new(),
            munition_catalog: BTreeMap::new(),
        };
        let out = build_mech(&unit, BTreeMap::new(), BTreeMap::new(), &idx).unwrap();
        assert_eq!(out.mech.weight_class, "Heavy");
        assert_eq!(out.mech.subtype, "BattleMek Omni");
    }
}
