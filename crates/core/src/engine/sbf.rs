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

//! Phase 2 of Strategic BattleForce support (see `docs/sbf-implementation-spec.md`): aggregate typed
//! Alpha Strike elements ([`AsElement`]) into an [`SbfUnit`] (a lance/star of 1–6 elements) and
//! [`SbfUnit`]s into an [`SbfFormation`] (1–4 units).
//!
//! This is a **faithful, bug-for-bug port** of MegaMek's `SBFUnitConverter.java` +
//! `BaseFormationConverter.java`, because Phase-2 golden tests compare against MegaMek's own
//! converter output. Where MegaMek is provably buggy or dead (turret SUAs invisible, the STL/MAS TMM
//! bonus that reads still-empty unit SUAs, the ATAC double-division, the always-taken transport
//! branch), we **reproduce the behavior and annotate it** — fixes, if any, belong to Phase 4 combat
//! where neurohelmet owns the rules. `convert_unit` reads only `AsElement.suas` (top-level), never
//! `AsElement.turret_suas`, exactly as MegaMek reads only the top-level SUA map.
//!
//! All rounding is Java `Math.round` = [`jround`] (half up); `(int)` casts truncate toward zero;
//! integer `/2` is floor.

use super::as_element::{jround, AsElement, DamageVector, SbfElementType, SuaVal};
use std::collections::BTreeMap;

/// SBF movement mode (`SBFMovementMode.java`). Lower [`SbfMoveMode::rank`] = more restrictive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SbfMoveMode {
    #[default]
    Unknown,
    BimAerospaceWalk,
    LamAerospaceWalk,
    MekWalk,
    Warship,
    BaWalk,
    Wheeled,
    Vtol,
    InfantryFoot,
    Airship,
    Rail,
    Hover,
    Wige,
    Spheroid,
    Aerodyne,
    QuadTracked,
    QuadWheeled,
    StationKeeping,
    Naval,
    Tracked,
    Submarine,
    MekUmu,
    BaUmu,
    MekJump,
    BaJump,
    CiJump,
}

impl SbfMoveMode {
    /// The printed movement-mode code (`SBFMovementMode.java:31-56`).
    pub fn code(self) -> &'static str {
        use SbfMoveMode::*;
        match self {
            Warship => "aw",
            Wheeled => "w",
            Vtol => "v",
            InfantryFoot => "f",
            Airship => "i",
            Rail => "r",
            Hover => "h",
            Wige => "g",
            Spheroid => "p",
            Aerodyne => "a",
            QuadTracked => "qt",
            QuadWheeled => "qw",
            StationKeeping => "k",
            Naval => "n",
            Tracked => "t",
            Submarine | MekUmu | BaUmu => "s",
            Unknown => "Unknown",
            // Walk/jump variants print blank.
            BimAerospaceWalk | LamAerospaceWalk | MekWalk | BaWalk | MekJump | BaJump | CiJump => "",
        }
    }

    /// Restrictiveness rank (`SBFMovementMode.java:31-56`); lower = more restrictive, `Unknown` = max.
    pub fn rank(self) -> i32 {
        use SbfMoveMode::*;
        match self {
            Naval | Submarine => 0,
            Rail => 10,
            StationKeeping => 11,
            Wheeled => 20,
            Spheroid => 21,
            Hover => 30,
            Warship => 31,
            Tracked => 40,
            Airship => 41,
            BimAerospaceWalk | BaWalk => 50,
            BaUmu => 51,
            InfantryFoot => 52,
            CiJump => 53,
            Aerodyne => 54,
            MekWalk => 60,
            BaJump => 61,
            MekUmu => 62,
            QuadTracked => 63,
            QuadWheeled => 64,
            LamAerospaceWalk => 65,
            MekJump => 70,
            Vtol => 80,
            Wige => 81,
            Unknown => i32::MAX,
        }
    }
}

/// The SBF movement mode for one element (`SBFMovementMode.modeForElement`, :68-141). Type qualifiers
/// match on `el.as_type` (the raw AS `tp`), not the collapsed `sbf_type`, mirroring MegaMek.
pub fn mode_for_element(unit_is_aero: bool, el: &AsElement) -> SbfMoveMode {
    use SbfMoveMode::*;
    if unit_is_aero && el.sbf_type.is_ground() {
        return if el.has_sua("BIM") {
            BimAerospaceWalk
        } else if el.has_sua("LAM") {
            LamAerospaceWalk
        } else if el.has_sua("SOA") {
            StationKeeping
        } else {
            Unknown
        };
    }
    let t = el.as_type.as_str();
    match el.primary_mode.as_str() {
        "" => {
            if matches!(t, "BM" | "PM" | "IM") {
                MekWalk
            } else if t == "WS" {
                Warship // unreachable in neurohelmet's catalog (no WS baked)
            } else if t == "BA" {
                BaWalk
            } else {
                Unknown
            }
        }
        "w" | "w(b)" | "w(m)" | "m" => Wheeled,
        "v" => Vtol,
        "f" => InfantryFoot,
        "i" => Airship,
        "r" => Rail,
        "h" => Hover,
        "g" => Wige,
        "p" => Spheroid,
        "a" => Aerodyne,
        "qt" => QuadTracked,
        "qw" => QuadWheeled,
        "k" => StationKeeping,
        "n" => Naval,
        "t" => Tracked,
        "s" => {
            if matches!(t, "CV" | "SV") {
                Submarine
            } else if matches!(t, "BM" | "PM") {
                MekUmu
            } else if t == "BA" {
                BaUmu
            } else {
                Unknown
            }
        }
        "j" => {
            if matches!(t, "BM" | "PM") {
                MekJump
            } else if t == "BA" {
                BaJump
            } else if t == "CI" {
                CiJump
            } else {
                Unknown
            }
        }
        _ => Unknown,
    }
}

/// Most-restrictive mode over an iterator: start `Unknown`, adopt any mode of **strictly** smaller
/// rank (so ties keep the first-seen), mirroring `setMovementMode` / `most_restrictive`.
fn most_restrictive(modes: impl Iterator<Item = SbfMoveMode>) -> SbfMoveMode {
    let mut cur = SbfMoveMode::Unknown;
    for m in modes {
        if m.rank() < cur.rank() {
            cur = m;
        }
    }
    cur
}

/// A converted SBF Unit — derived, immutable stats. Live combat state (armor hits, crits) lives on
/// the Phase-3 session, never here; recompute this on demand.
#[derive(Clone, Debug, PartialEq)]
pub struct SbfUnit {
    pub name: String,
    pub sbf_type: SbfElementType,
    pub size: u8,
    pub movement: i64,
    pub move_mode: SbfMoveMode,
    pub jump_move: i64,
    pub trsp_movement: i64,
    pub trsp_mode: SbfMoveMode,
    pub tmm: i64,
    pub armor: i64,
    pub damage: DamageVector,
    pub skill: i64,
    pub point_value: i64,
    pub suas: BTreeMap<String, SuaVal>,
}

/// A converted SBF Formation (1–4 units).
#[derive(Clone, Debug, PartialEq)]
pub struct SbfFormation {
    pub name: String,
    pub sbf_type: SbfElementType,
    pub size: i64,
    pub tmm: i64,
    pub movement: i64,
    pub move_mode: SbfMoveMode,
    pub jump_move: i64,
    pub trsp_movement: i64,
    pub trsp_mode: SbfMoveMode,
    pub tactics: i64,
    pub morale_rating: i64,
    pub skill: i64,
    pub point_value: i64,
    pub suas: BTreeMap<String, SuaVal>,
    pub units: Vec<SbfUnit>,
}

impl SbfFormation {
    /// NARROW formation aerospace test (`SBFFormation.java:287-290`): `As` or `La` ONLY — distinct
    /// from the broad `SbfElementType::is_aerospace()`. Governs formation Movement min-vs-mean (§2.4).
    pub fn is_aerospace(&self) -> bool {
        matches!(self.sbf_type, SbfElementType::As | SbfElementType::La)
    }
}

/// Firing range bracket, chosen by hand (no board). Also used by the Phase-4 `DamageVector::band`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SbfRange {
    Short,
    #[default]
    Medium,
    Long,
    Extreme,
}

impl DamageVector {
    /// The damage at a firing-range bracket. Extreme is a legal SBF attack (+3, IO:BF p.172);
    /// a vector without an E band deals `L − 1` there (Step 5a: "Extreme Range damage is equal
    /// to Long Range −1"), floored at 0.
    pub fn band(&self, r: SbfRange) -> f32 {
        match r {
            SbfRange::Short => self.s,
            SbfRange::Medium => self.m,
            SbfRange::Long => self.l.unwrap_or(0.0),
            SbfRange::Extreme => self
                .e
                .unwrap_or((self.l.unwrap_or(0.0) - 1.0).max(0.0)),
        }
    }
}

/// Reduce every band by `n` (each damage crit is −1 at every range), floored at 0. Mirrors
/// `ASDamageVector.reducedBy` (:376-387); `getCurrentDamage` = `damage.reducedBy(damageCrits)`.
pub fn reduced_by(v: DamageVector, n: u8) -> DamageVector {
    let r = |x: f32| (x - n as f32).max(0.0);
    DamageVector { s: r(v.s), m: r(v.m), l: v.l.map(r), e: v.e.map(r) }
}

// ---- SUA aggregation vocabularies (order matters where noted) ----
const UNIT_IF_ANY: &[&str] = &[
    "WAT", "PRB", "AECM", "BHJ2", "BHJ3", "BH", "BT", "ECM", "HPG", "LPRB", "LECM", "TAG",
];
const UNIT_IF_HALF: &[&str] = &["AMS", "ARM", "ARS", "BAR", "BFC", "CR", "ENG", "RBT", "SRCH", "SHLD"];
const UNIT_IF_ALL: &[&str] = &["AMP", "AM", "BHJ", "XMEC", "MCS", "UCS", "MEC", "PAR", "SAW", "TRN"];
const UNIT_SUM: &[&str] = &[
    "CAR", "CK", "CT", "IT", "CRW", "DCC", "MDS", "MASH", "RSD", "VTM", "VTH", "VTS", "AT", "DT",
    "MT", "PT", "ST", "SCR",
];
const UNIT_SUM_DIV3: &[&str] = &["ATAC", "BOMB", "PNT", "IF"];
const UNIT_ARTILLERY: &[&str] = &[
    "ARTLT", "ARTS", "ARTT", "ARTBA", "ARTCM5", "ARTCM7", "ARTCM9", "ARTCM12",
];

const FORM_IF_ANY: &[&str] = &[
    "DN", "XMEC", "COM", "HPG", "MCS", "UCS", "MEC", "MAS", "LMAS", "MSW", "MFB", "SAW", "SDS",
    "TRN", "FD", "HELI", "SDCS",
];
const FORM_IF_2_3: &[&str] = &[
    "AC3", "PRB", "AECM", "ECM", "ENG", "LPRB", "LECM", "ORO", "RCN", "SRCH", "SHLD", "TAG", "WAT",
];
const FORM_IF_ALL: &[&str] = &["AMP", "BH", "EE", "FC", "SEAL", "MAG", "PAR", "RAIL", "RBT", "UMU"];
const FORM_SUM: &[&str] = &[
    "SBF_OMNI", "CAR", "CK", "CT", "IT", "CRW", "DCC", "MDS", "MASH", "RSD", "VTM", "VTH", "VTS",
    "AT", "BOMB", "DT", "MT", "PT", "ST", "SCR", "PNT", "IF", "MHQ",
];

/// Artillery damage table (`SBFFormation.getSbfArtilleryDamage`, :233-259).
pub fn art_damage(code: &str) -> i64 {
    match code {
        "ARTTC" => 1,
        "ARTT" | "ARTBA" | "ARTSC" => 2,
        "ARTAIS" | "ARTAC" | "ARTS" | "ARTLTC" => 3,
        "ARTLT" => 6,
        "ARTCM5" => 8,
        "ARTCM7" => 13,
        "ARTCM9" => 22,
        "ARTCM12" => 36,
        _ => 0,
    }
}

/// The numeric value of a resolved SUA (for aggregation): `Num→v`, `Art→c`, `Dmg→S`, `Flag→1`
/// (MegaMek's `null value → +1` merge), `Lam`/`Bim → 0`.
pub(crate) fn suaval_num(v: &SuaVal) -> f64 {
    match v {
        SuaVal::Num(x) => *x as f64,
        SuaVal::Art(c) => *c as f64,
        SuaVal::Dmg(d) => d.s as f64,
        SuaVal::Flag => 1.0,
        _ => 0.0,
    }
}

pub(crate) fn mean(vals: impl Iterator<Item = f64>) -> f64 {
    let (sum, n) = vals.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
    if n == 0 {
        0.0
    } else {
        sum / n as f64
    }
}

/// The "most frequent, first-seen at the max count; MX if below the 2/3 majority" type rule shared
/// by unit conversion (over element types) and formation conversion (over unit types).
pub(crate) fn majority_type(types: &[SbfElementType]) -> SbfElementType {
    let majority = jround(2.0 / 3.0 * types.len() as f64);
    let highest_count = types
        .iter()
        .map(|t| types.iter().filter(|x| *x == t).count())
        .max()
        .unwrap_or(0);
    let highest_type = types
        .iter()
        .copied()
        .find(|t| types.iter().filter(|x| *x == t).count() == highest_count)
        .unwrap_or(SbfElementType::Unknown);
    if (highest_count as i64) < majority {
        SbfElementType::Mx
    } else {
        highest_type
    }
}

fn tmm_from_move(m: i64) -> i64 {
    if m >= 18 {
        5
    } else if m >= 10 {
        4
    } else if m >= 7 {
        3
    } else if m >= 5 {
        2
    } else if m >= 3 {
        1
    } else if m >= 1 {
        0
    } else {
        -4
    }
}

fn effective_thrust(e: &AsElement) -> i64 {
    if e.has_sua("SOA") {
        0
    } else {
        e.primary_move as i64
    }
}

fn is_infantry(e: &AsElement) -> bool {
    matches!(e.sbf_type, SbfElementType::Ci | SbfElementType::Ba)
}

/// `isConsideredRcn` (`SBFUnitConverter.java:479-487`). The `IMPROVED_SENSORS` quirk clause is
/// dropped (quirks are not baked; flagged NEEDS RULEBOOK / data).
fn is_considered_rcn(e: &AsElement) -> bool {
    (e.as_type == "BM" && e.size <= 2 && e.primary_move >= 14)
        || ((e.as_type == "BM" || e.as_type == "PM") && e.jump_move >= 12)
        || (e.sbf_type.is_ground() && e.size <= 2 && e.primary_move >= 18)
        || e.name.contains("Scout")
        || e.name.contains("Recon")
        || e.name.contains("Sensor")
}

/// Convert 1–6 Alpha Strike elements into an SBF Unit (`SBFUnitConverter.createSbfUnit`, :55-80).
/// Order of operations is load-bearing: TMM and transport (which read unit SUAs) run *before* the
/// SUA aggregation, so those reads see an empty map — reproduced as dead per MegaMek.
pub fn convert_unit(name: &str, elems: &[AsElement]) -> SbfUnit {
    let n = elems.len();
    let count = |code: &str| elems.iter().filter(|e| e.has_sua(code)).count() as i64;

    // (1) Type.
    let types: Vec<SbfElementType> = elems.iter().map(|e| e.sbf_type).collect();
    let unit_type = majority_type(&types);
    let unit_is_aero = unit_type.is_aerospace();

    // (2) Size.
    let size = jround(mean(elems.iter().map(|e| e.size as f64))) as u8;

    // (3) Movement (sets rounded_average_move). BROAD aerospace test here (SBFUnit.isAerospace).
    let mut rounded_average_move = 0i64;
    let movement;
    if unit_is_aero {
        movement = elems.iter().map(effective_thrust).min().unwrap_or(0);
        // rounded_average_move stays 0 → TMM = getTmmFromMove(0) = -4 (NEEDS RULEBOOK, aero TMM).
    } else {
        rounded_average_move = jround(mean(elems.iter().map(|e| e.primary_move as f64)) / 2.0);
        if elems.iter().any(is_infantry) {
            let min_inf = elems
                .iter()
                .filter(|e| is_infantry(e))
                .map(|e| e.primary_move)
                .min()
                .unwrap_or(0) as i64
                / 2; // integer floor — diverges from the rounded average
            movement = rounded_average_move.min(min_inf);
        } else {
            movement = rounded_average_move;
        }
    }

    // (4) Movement mode.
    let move_mode = most_restrictive(elems.iter().map(|e| mode_for_element(unit_is_aero, e)));

    // (5) Jump.
    let jump_move = jround(mean(elems.iter().map(|e| e.jump_move as f64)) / 4.0);

    // (6) Transport move + mode.
    let non_transportable: Vec<&AsElement> = elems
        .iter()
        .filter(|e| !e.has_any_sua(&["MEC", "XMEC", "CAR"]))
        .collect();
    let has_transportable = elems.iter().any(|e| e.has_any_sua(&["MEC", "XMEC", "CAR"]));
    let (trsp_movement, trsp_mode);
    if non_transportable.is_empty() || !has_transportable {
        // branch 1/2: no transportable, or nothing to transport → mirror primary move.
        trsp_movement = movement;
        trsp_mode = move_mode;
    } else {
        // branch 3: unit.CAR <= unit.IT — both are still 0 (unit SUAs not set) → always taken.
        // (branch 4, "only some elements transportable", is a TBD stub in MegaMek — NEEDS RULEBOOK.)
        let avg = mean(non_transportable.iter().map(|e| e.primary_move as f64));
        trsp_movement = jround(avg / 2.0);
        trsp_mode = most_restrictive(
            non_transportable
                .iter()
                .map(|e| mode_for_element(unit_is_aero, e)),
        );
    }

    // (7) TMM — keys off rounded_average_move, not movement.
    let mut tmm = tmm_from_move(rounded_average_move);
    if matches!(unit_type, SbfElementType::Ba | SbfElementType::Pm)
        || (unit_type == SbfElementType::V && matches!(move_mode.code(), "v" | "g"))
    {
        tmm += 1;
    }
    if elems.iter().any(|e| e.has_sua("LG")) {
        tmm -= 1;
    }
    if elems.iter().any(|e| e.has_any_sua(&["SLG", "VLG"])) {
        tmm -= 2;
    }
    // (STL/MAS +2 reads unit SUAs, which are still empty at this step → never fires. Dead by design.)

    // (8) Armor.
    let mut armor_sum = 0.0f64;
    for e in elems {
        let mut v = (e.full_armor + e.full_structure) as f64;
        let mut delta = 0.0;
        if e.full_structure >= 3 || e.has_any_sua(&["AMS", "CASE"]) {
            delta = 0.5;
        }
        if e.has_any_sua(&["ENE", "CASEII", "CR", "RAMS"]) {
            delta = 1.0; // overwrites 0.5; never 1.5
        }
        v += delta;
        armor_sum += v;
    }
    let armor = jround(armor_sum / 3.0);

    // (9) Unit SUA aggregation (populates `suas`), in MegaMek's exact order.
    let mut suas: BTreeMap<String, SuaVal> = BTreeMap::new();
    // (a) IfAny.
    for &code in UNIT_IF_ANY {
        if count(code) >= 1 {
            suas.insert(code.to_string(), SuaVal::Flag);
        }
    }
    // (b) IfHalf: max(1, n/2) floor.
    let half = (n / 2).max(1) as i64;
    for &code in UNIT_IF_HALF {
        if count(code) >= half {
            suas.insert(code.to_string(), SuaVal::Flag);
        }
    }
    // (c) IfAll.
    for &code in UNIT_IF_ALL {
        if count(code) >= n as i64 {
            suas.insert(code.to_string(), SuaVal::Flag);
        }
    }
    // (d) sum (plain).
    for &code in UNIT_SUM {
        let mut sum = 0.0;
        let mut any = false;
        for e in elems {
            if e.has_sua(code) {
                any = true;
                sum += e.sua_num(code) as f64;
            }
        }
        if any {
            suas.insert(code.to_string(), SuaVal::Num(sum as f32));
        }
    }
    // (e) sumDivideBy3. IF is ASDamage-valued → use the INTEGER `.damage` (0* → 0) via floor.
    for &code in UNIT_SUM_DIV3 {
        let sum: f64 = elems
            .iter()
            .filter(|e| e.has_sua(code))
            .map(|e| (e.sua_num(code) as f64).floor())
            .sum();
        if sum > 0.0 {
            let one_third = jround(sum / 3.0);
            if one_third > 0 {
                suas.insert(code.to_string(), SuaVal::Num(one_third as f32));
            }
        }
    }
    // (f) sumArtillery.
    for &code in UNIT_ARTILLERY {
        let c: i64 = elems
            .iter()
            .filter(|e| e.has_sua(code))
            .map(|e| e.sua_num(code) as i64)
            .sum();
        let value = jround((c * art_damage(code)) as f64 / 3.0);
        if value > 0 {
            suas.insert(code.to_string(), SuaVal::Num(value as f32));
        }
    }
    // (g) MHQ: Σ(value − 1) per element, /3.
    if elems.iter().any(|e| e.has_sua("MHQ")) {
        let total: f64 = elems
            .iter()
            .filter(|e| e.has_sua("MHQ"))
            .map(|e| e.sua_num("MHQ") as f64 - 1.0)
            .sum();
        let one_third = jround(total / 3.0);
        if one_third > 0 {
            suas.insert("MHQ".to_string(), SuaVal::Num(one_third as f32));
        }
    }
    // (h) RCN.
    if elems
        .iter()
        .filter(|e| e.has_sua("RCN") || is_considered_rcn(e))
        .count()
        >= 2
    {
        suas.insert("RCN".to_string(), SuaVal::Flag);
    }
    // (i) STL: every element has STL/MAS/LMAS → set all three.
    if !elems.is_empty() && elems.iter().all(|e| e.has_any_sua(&["STL", "MAS", "LMAS"])) {
        suas.insert("STL".to_string(), SuaVal::Flag);
        suas.insert("MAS".to_string(), SuaVal::Flag);
        suas.insert("LMAS".to_string(), SuaVal::Flag);
    }
    // (j) OMNI → SBF_OMNI count.
    let omni = count("OMNI");
    if omni > 0 {
        suas.insert("SBF_OMNI".to_string(), SuaVal::Num(omni as f32));
    }
    // (k) FLK: from top-level FLK + AC vectors only.
    let flk_band = |band: fn(&DamageVector) -> f64| -> i64 {
        let s: f64 = elems
            .iter()
            .filter_map(|e| e.sua_dmg("FLK"))
            .map(|d| band(&d))
            .sum::<f64>()
            + elems
                .iter()
                .filter_map(|e| e.sua_dmg("AC"))
                .map(|d| band(&d))
                .sum::<f64>();
        jround(s / 3.0)
    };
    let flk_m = flk_band(|d| d.m as f64);
    let flk_l = flk_band(|d| d.l.unwrap_or(0.0) as f64);
    if flk_m + flk_l > 0 {
        suas.insert(
            "FLK".to_string(),
            SuaVal::Dmg(DamageVector {
                s: 0.0,
                m: flk_m as f32,
                l: Some(flk_l as f32),
                e: None,
            }),
        );
    }
    // (l) C3M/C3BSM → AC3.
    if (count("C3M") >= 1 || count("C3BSM") >= 1)
        && count("C3M") + count("C3S") + count("C3BSS") >= n as i64 / 2
    {
        suas.insert("AC3".to_string(), SuaVal::Flag);
    }
    // (m) C3I → AC3.
    if count("C3I") > 0 && count("C3I") >= n as i64 / 2 {
        suas.insert("AC3".to_string(), SuaVal::Flag);
    }
    // (n) FUEL: min over aerospace elements.
    if let Some(f) = elems
        .iter()
        .filter(|e| e.sbf_type.is_aerospace())
        .map(|e| e.fuel_rating() as i64)
        .min()
    {
        suas.insert("FUEL".to_string(), SuaVal::Num(f as f32));
    }
    // (o) ATAC /3 again — double division (net ≈ sum/9). Reproduced for parity.
    if let Some(v) = suas.get("ATAC").map(suaval_num) {
        suas.insert("ATAC".to_string(), SuaVal::Num(jround(v / 3.0) as f32));
    }
    // (p) finalize: removals/merges based on what is set.
    if suas.contains_key("PRB") {
        suas.remove("LPRB");
    }
    if suas.contains_key("AECM") {
        suas.remove("LECM");
        suas.remove("ECM");
    }
    if suas.contains_key("ECM") {
        suas.remove("LECM");
    }
    if let Some(ct) = suas.get("CT").map(suaval_num) {
        let it = suas.get("IT").map(suaval_num).unwrap_or(0.0) + ct;
        suas.insert("IT".to_string(), SuaVal::Num(it as f32));
        suas.remove("CT");
    }

    // (10) Damage.
    let art_tc = count("ARTTC") as f64 * art_damage("ARTTC") as f64;
    let art_ltc = count("ARTLTC") as f64 * art_damage("ARTLTC") as f64;
    let art_sc = count("ARTSC") as f64 * art_damage("ARTSC") as f64;
    let art_smj = art_tc + art_ltc + art_sc; // S and M bands
    let art_l = art_tc + art_ltc; // L and E bands (artSC excluded)

    // S band.
    let mut dmg_s: f64 = elems.iter().map(|e| e.std_damage.s as f64).sum();
    let ov: f64 = elems.iter().map(|e| e.get_ov() as f64).sum::<f64>() / 2.0;
    if ov > 0.0 {
        dmg_s += ov;
    }
    if matches!(unit_type, SbfElementType::Ba | SbfElementType::Ci) && suas.contains_key("AM") {
        dmg_s += 1.0;
    }
    if art_smj > 0.0 {
        dmg_s += art_smj;
    }
    let dmg_s = jround(dmg_s / 3.0);

    // M band (no AM); OV only from elements whose M damage is a real ≥1 (0* excluded).
    let mut dmg_m: f64 = elems.iter().map(|e| e.std_damage.m as f64).sum();
    let ov_m: f64 = elems
        .iter()
        .filter(|e| e.std_damage.m >= 1.0)
        .map(|e| e.get_ov() as f64)
        .sum::<f64>()
        / 2.0;
    if ov_m > 0.0 {
        dmg_m += ov_m;
    }
    if art_smj > 0.0 {
        dmg_m += art_smj;
    }
    let dmg_m = jround(dmg_m / 3.0);

    // L band; OV only from OVL elements with real ≥1 L damage.
    let mut dmg_l: f64 = elems.iter().map(|e| e.std_damage.l.unwrap_or(0.0) as f64).sum();
    let ov_l: f64 = elems
        .iter()
        .filter(|e| e.has_sua("OVL") && e.std_damage.l.unwrap_or(0.0) >= 1.0)
        .map(|e| e.get_ov() as f64)
        .sum::<f64>()
        / 2.0;
    if ov_l > 0.0 {
        dmg_l += ov_l;
    }
    if art_l > 0.0 {
        dmg_l += art_l;
    }
    let dmg_l = jround(dmg_l / 3.0);

    let damage = if unit_type == SbfElementType::As {
        let mut dmg_e: f64 = elems.iter().map(|e| e.std_damage.e.unwrap_or(0.0) as f64).sum();
        if art_l > 0.0 {
            dmg_e += art_l;
        }
        DamageVector {
            s: dmg_s as f32,
            m: dmg_m as f32,
            l: Some(dmg_l as f32),
            e: Some(jround(dmg_e / 3.0) as f32),
        }
    } else {
        DamageVector {
            s: dmg_s as f32,
            m: dmg_m as f32,
            l: Some(dmg_l as f32),
            e: None,
        }
    };

    // (11) Skill.
    let mut skill = if n == 0 {
        4
    } else {
        jround(mean(elems.iter().map(|e| e.skill as f64)))
    };
    if suas.contains_key("DN") {
        skill -= 1;
    }
    if ["BFC", "DRO", "RBT"].iter().any(|c| suas.contains_key(*c)) {
        skill += 1;
    }
    let skill = skill.clamp(0, 7);

    // (12) Point value.
    let intermediate = jround(elems.iter().map(|e| e.base_pv as f64).sum::<f64>() / 3.0);
    let mut result = intermediate as f64;
    if skill > 4 {
        result = (1.0 - (skill - 4) as f64 * 0.1) * intermediate as f64;
    } else if skill < 4 {
        result = (1.0 + (4 - skill) as f64 * 0.2) * intermediate as f64;
        result = result.max((intermediate + (4 - skill)) as f64);
    }
    let point_value = jround(result).max(1);

    SbfUnit {
        name: name.to_string(),
        sbf_type: unit_type,
        size,
        movement,
        move_mode,
        jump_move,
        trsp_movement,
        trsp_mode,
        tmm,
        armor,
        damage,
        skill,
        point_value,
        suas,
    }
}

/// Convert 1–4 SBF Units into an SBF Formation (`BaseFormationConverter.calcSbfFormationStats`).
pub fn convert_formation(name: &str, units: &[SbfUnit]) -> SbfFormation {
    let spa_count = |code: &str| units.iter().filter(|u| u.suas.contains_key(code)).count() as i64;

    let types: Vec<SbfElementType> = units.iter().map(|u| u.sbf_type).collect();
    let sbf_type = majority_type(&types);

    let size = jround(mean(units.iter().map(|u| u.size as f64)));

    // Movement: mean, or MIN when the formation is aerospace (NARROW test on the derived type).
    let is_aero = matches!(sbf_type, SbfElementType::As | SbfElementType::La);
    let movement = if is_aero {
        jround(units.iter().map(|u| u.movement).min().unwrap_or(0) as f64)
    } else {
        jround(mean(units.iter().map(|u| u.movement as f64)))
    };
    let move_mode = most_restrictive(units.iter().map(|u| u.move_mode));
    let trsp_movement = jround(mean(units.iter().map(|u| u.trsp_movement as f64)));
    let trsp_mode = most_restrictive(units.iter().map(|u| u.trsp_mode));
    let jump_move = jround(mean(units.iter().map(|u| u.jump_move as f64)));
    let tmm = jround(mean(units.iter().map(|u| u.tmm as f64)));
    let skill = jround(mean(units.iter().map(|u| u.skill as f64)));
    let morale_rating = 3 + skill;

    // Formation SUAs.
    let mut suas: BTreeMap<String, SuaVal> = BTreeMap::new();
    for &code in FORM_IF_ANY {
        if spa_count(code) >= 1 {
            suas.insert(code.to_string(), SuaVal::Flag);
        }
    }
    let two_thirds = (units.len() as i64 - 1).max(1); // "all but one", NOT literal 2/3
    for &code in FORM_IF_2_3 {
        if spa_count(code) >= two_thirds {
            suas.insert(code.to_string(), SuaVal::Flag);
        }
    }
    for &code in FORM_IF_ALL {
        if spa_count(code) >= units.len() as i64 {
            suas.insert(code.to_string(), SuaVal::Flag);
        }
    }
    for &code in FORM_SUM {
        let sum: f64 = units
            .iter()
            .filter_map(|u| u.suas.get(code))
            .map(suaval_num)
            .sum();
        if sum > 0.0 {
            suas.insert(code.to_string(), SuaVal::Num((sum as i64) as f32)); // (int) truncate
        }
    }
    // FUEL: min over aerospace units (getFUEL is 0 when absent).
    if let Some(f) = units
        .iter()
        .filter(|u| u.sbf_type.is_aerospace())
        .map(|u| u.suas.get("FUEL").map(suaval_num).unwrap_or(0.0) as i64)
        .min()
    {
        suas.insert("FUEL".to_string(), SuaVal::Num(f as f32));
    }
    // CAR ↔ IT cancel (both re-stored even at 0).
    if suas.contains_key("CAR") && suas.contains_key("IT") {
        let car = suas.get("CAR").map(suaval_num).unwrap_or(0.0);
        let it = suas.get("IT").map(suaval_num).unwrap_or(0.0);
        suas.insert("CAR".to_string(), SuaVal::Num((car - it).max(0.0) as f32));
        suas.insert("IT".to_string(), SuaVal::Num((it - car).max(0.0) as f32));
    }
    // IF is already an integer Num (matches MegaMek re-wrapping it as ASDamage(int)).

    // Tactics (no re-clamp after the MHQ subtraction — can go slightly negative).
    let mut tactics = (10 - movement + skill - 4).max(0);
    if let Some(mhq) = suas.get("MHQ").map(suaval_num) {
        tactics -= 3.min(jround(mhq / 2.0));
    }

    let point_value = units.iter().map(|u| u.point_value).sum();

    SbfFormation {
        name: name.to_string(),
        sbf_type,
        size,
        tmm,
        movement,
        move_mode,
        jump_move,
        trsp_movement,
        trsp_mode,
        tactics,
        morale_rating,
        skill,
        point_value,
        suas,
        units: units.to_vec(),
    }
}

// ============================ Phase 4 — combat resolution ============================
// docs/sbf-implementation-spec.md §4. neurohelmet is a MANUAL tracker: everything below is a pure
// calculator or reference table — the engine rolls nothing (cf. `dice::cluster_hits`); the player
// supplies each 2d6 total. Single-force scope: the target is always hand-entered (no `with_target`
// tracked-OpFor lookup), and morale is a manual rung that is NOT a term in the to-hit number
// (§4.1/§4.3). Live damage/crit counters and the spillover/crippling/turn helpers that need the
// element pool live on `SbfUnitState`/`SbfState`/`Session` in `session.rs`.

/// Hand-entered to-hit context — the printed To-Hit Modifiers Table, IO:BF p.172 (§4.1), plus the
/// optional Strategic Aerospace leg (the p.179 table — [`SbfAeroShot`]). Only
/// fields that affect the number live here; the target's morale rung is deliberately absent
/// (manual morale, §4.3 — the printed −1/−2/−3 demoralized-target modifier is a deferred
/// decision). Also omitted as too niche for the tracker (documented §4.1): artillery-attack
/// (+1/ART this turn) and dismounted-from-transport (+1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SbfToHitCtx {
    /// Firing FORMATION skill (the base to-hit number).
    pub attacker_skill: i64,
    /// Targeting crits on the *firing unit*: "+1 per Critical".
    pub firing_unit_targeting_crits: u8,
    /// Player-chosen firing range bracket: +0 / +1 / +2 / +3 (Extreme is a legal attack).
    pub range: SbfRange,
    /// Indirect Fire attack: an additional +1 on top of the range modifier (L/E only — validity
    /// is the table's concern, the calculator just adds it).
    pub indirect_fire: bool,
    /// JUMP points the firing formation used this turn: +1 per point.
    pub attacker_jump: u8,
    /// Firing units withholding fire (that could damage the target at this range): −1 each,
    /// floored at −2 total.
    pub withheld_units: u8,
    /// Firing formation has the BFC special: +1 (derived from the formation SUAs).
    pub bfc: bool,
    /// Firing formation is a drone (DRO): +1 (derived).
    pub drone: bool,
    /// Firing formation is also spotting for Indirect Fire this turn: +1.
    pub spotting: bool,
    /// This attack is against a secondary target: +1.
    pub secondary: bool,
    /// Target formation TMM (>0 adds; a negative TMM is ignored).
    pub target_tmm: i64,
    /// JUMP points the target formation used: +1 per point.
    pub target_jump: u8,
    /// Target successfully Evaded: +1.
    pub target_evaded: bool,
    /// Terrain modifier, hand-entered (woods +1/+2, urban +1/+2, underwater +1).
    pub terrain: i64,
    /// The Strategic Aerospace leg (IO:BF pp.179–181): `Some` prices the shot off the Aerospace
    /// To-Hit Modifiers Table instead of the ground p.172 rows it replaces — see [`SbfAeroShot`].
    /// Ephemeral like the rest of the ctx; `None` is a plain ground shot.
    pub aero: Option<SbfAeroShot>,
}

// ---- Strategic Aerospace (SAS) shot leg — the p.179 Aerospace To-Hit Modifiers Table ----

/// Air-to-ground attack type (p.179 table + p.180 "Types of Attacks"). The Cluster Bomb −1 is a
/// table row modifying a bombing attack (Cluster bombs deal 1 damage vs HE's 2, p.180), not an
/// attack type of its own — it rides as `cluster` on the bombing kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SbfA2G {
    /// +3 — ≥1 BOMB point per hex along the flight path, one roll per hex (BA may not; p.180).
    AltitudeBombing { cluster: bool },
    /// +2 — one Formation, one roll per 5 bombs dropped (p.180).
    DiveBombing { cluster: bool },
    /// +4 — up to 4 formations along the path, per-Flight rolls, ¼ Short damage (p.180). (Note
    /// Strategic Aerospace strafing is +4 where Standard BF's is +2 — different scale, both
    /// correct.)
    Strafing,
    /// +2 — one Formation, full Short damage (p.180).
    Striking,
}

/// Which side of the SAS combat rules a shot is resolved under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SbfAeroKind {
    /// Squadron vs Squadron on the Atmospheric Radar Map (p.179).
    AirToAir,
    /// A ground Formation firing at an airborne Squadron crossing the Central Zone (p.181) — or
    /// at a grounded Squadron (the −4 immobile target row, p.181).
    GroundToAir,
    /// A Squadron in the Central Zone attacking the ground map (p.180).
    A2G(SbfA2G),
}

impl SbfAeroKind {
    /// The A2G attack-type row (p.179): Altitude Bombing +3 / Dive Bombing +2 / Strafing +4 /
    /// Striking +2, Cluster Bomb −1. Air-to-air and ground-to-air have no attack-type row.
    pub fn attack_mod(self) -> i64 {
        match self {
            Self::AirToAir | Self::GroundToAir => 0,
            Self::A2G(a) => match a {
                SbfA2G::AltitudeBombing { cluster } => 3 - i64::from(cluster),
                SbfA2G::DiveBombing { cluster } => 2 - i64::from(cluster),
                SbfA2G::Strafing => 4,
                SbfA2G::Striking => 2,
            },
        }
    }

    /// Whether this kind suppresses the ctx's target-movement and terrain legs (`target_tmm`,
    /// `target_jump`, `target_evaded`, `terrain`): air-to-air attacks "do not apply modifiers for
    /// the target's movement or terrain" (p.179); ground-to-air replaces the movement modifier
    /// with the flat airborne target row (p.181); bombing "do[es] not apply modifiers for the
    /// target's movement, type, or terrain" (p.180). Strafing/striking keep them — Open Q 25,
    /// DECIDED: the p.180 Step-3 "all other air-to-ground attacks must apply these modifiers"
    /// wins over the Targeting paragraph's blanket no-TMM line.
    pub fn suppresses_target_movement(self) -> bool {
        !matches!(self, Self::A2G(SbfA2G::Strafing | SbfA2G::Striking))
    }
}

/// Target-type rows of the p.179 table. One value per shot — the airborne-aerospace class rows
/// stack inside [`SbfAeroShot::target_mod`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SbfAeroTarget {
    /// Airborne aerospace Squadron: +2, gated on the attacker (see the p.179 fn).
    #[default]
    AirborneAero,
    /// Airborne DropShip: the −2 row on top of the gated +2 (p.181: ground-to-air vs a DropShip
    /// is "effectively a +0" — the rows stack; the fn folds DropShips into airborne aerospace).
    AirborneDropship,
    /// Airborne VTOL or WiGE: +1 (not aerospace — no +2 underneath).
    AirborneVtolWige,
    /// Small Craft: the −1 row on top of the gated +2 (the fn folds small craft into airborne
    /// aerospace too).
    SmallCraft,
    /// Grounded aerospace Squadron: treated as a ground Formation but with a −4 immobile target
    /// modifier in place of target movement (p.181).
    GroundedSquadron,
    /// A ground Formation (the A2G case): no target-type row.
    GroundFormation,
}

/// The p.179 "Attacker is Support Vehicle with:" fire-control rows. Non-SV attackers use [`Self::Afc`]
/// (+0) — the row only penalizes support vehicles lacking advanced fire control, and BFC-bearing
/// non-SV formations still price their p.172 "Has the BFC special +1" through [`Self::Bfc`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SbfSvFireControl {
    /// Advanced Fire Control: +0 (also "row not applicable").
    #[default]
    Afc,
    /// Basic Fire Control: +1.
    Bfc,
    /// A support vehicle with no AFC or BFC special: +2.
    None,
}

impl SbfSvFireControl {
    /// The fire-control row value (p.179): AFC +0 / BFC +1 / neither +2.
    pub fn to_hit_mod(self) -> i64 {
        match self {
            Self::Afc => 0,
            Self::Bfc => 1,
            Self::None => 2,
        }
    }
}

/// The Strategic Aerospace leg of a shot — the p.179 Aerospace To-Hit Modifiers Table, hand-entered
/// like the rest of [`SbfToHitCtx`] (the radar-map positional procedure that *produces* these facts
/// — engagement control, tailing, flight paths — stays at the table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SbfAeroShot {
    pub kind: SbfAeroKind,
    pub target: SbfAeroTarget,
    /// Gates the +2 airborne-aerospace target row: "Apply only if attacker is not an airborne
    /// aerospace Squadron" (p.179 fn). Derived from the firing formation's type.
    pub attacker_airborne_aero: bool,
    /// Attacker is "behind" the target: −2 (p.179 Misc).
    pub behind_target: bool,
    /// Attacker is a grounded DropShip: −2 (p.179 Misc). Unreachable from the baked catalog
    /// (large-craft AS types are not baked — spec Open Q 20); the row is priced anyway.
    pub grounded_dropship: bool,
    /// The SV fire-control row (p.179 Misc) — supersedes the ctx's ground `bfc` row (both price
    /// the same fire control; applying both would double-count BFC).
    pub sv_fire_control: SbfSvFireControl,
}

impl SbfAeroShot {
    /// The target-type rows (p.179). The +2 airborne-aerospace row is gated on the attacker not
    /// being an airborne aerospace Squadron (fn); the DropShip −2 and Small Craft −1 rows stack
    /// on it — the fn folds both into "airborne aerospace", and p.181 confirms the stack
    /// (ground-to-air vs an airborne DropShip is "effectively a +0").
    pub fn target_mod(&self) -> i64 {
        let airborne_aero = if self.attacker_airborne_aero { 0 } else { 2 };
        match self.target {
            SbfAeroTarget::AirborneAero => airborne_aero,
            SbfAeroTarget::AirborneDropship => airborne_aero - 2,
            SbfAeroTarget::AirborneVtolWige => 1,
            SbfAeroTarget::SmallCraft => airborne_aero - 1,
            SbfAeroTarget::GroundedSquadron => -4,
            SbfAeroTarget::GroundFormation => 0,
        }
    }

    /// The Misc attacker rows (p.179): behind the target −2, grounded DropShip −2, and the SV
    /// fire-control ladder.
    pub fn misc_mod(&self) -> i64 {
        -2 * i64::from(self.behind_target) - 2 * i64::from(self.grounded_dropship)
            + self.sv_fire_control.to_hit_mod()
    }
}

/// Range-bracket to-hit modifier — the printed ladder (IO:BF p.172): S +0, M +1, L +2, E +3.
pub fn sbf_range_mod(r: SbfRange) -> i64 {
    match r {
        SbfRange::Short => 0,
        SbfRange::Medium => 1,
        SbfRange::Long => 2,
        SbfRange::Extreme => 3,
    }
}

/// SBF to-hit target number (§4.1, the p.172 table; with an [`SbfToHitCtx::aero`] leg, the p.179
/// Aerospace To-Hit Modifiers Table): hit iff `2d6 >= n` ("equals or exceeds", Step 4 — no
/// natural-2/12 auto results). Each firing unit rolls against this number.
pub fn sbf_to_hit(atk: &SbfToHitCtx) -> i64 {
    let mut n = atk.attacker_skill
        + atk.firing_unit_targeting_crits as i64
        + sbf_range_mod(atk.range)
        + atk.indirect_fire as i64
        + atk.attacker_jump as i64
        - (atk.withheld_units as i64).min(2) // −1 per withholding unit, max −2
        + atk.drone as i64 // +1 on both tables (p.172 / p.179 Misc) — never doubled
        + atk.spotting as i64
        + atk.secondary as i64;
    // Fire control: the p.172 row is "Has the BFC special +1"; an aero shot prices fire control
    // through the p.179 SV ladder instead (AFC +0 / BFC +1 / neither +2, inside `misc_mod`) —
    // only one may apply.
    if atk.aero.is_none() {
        n += atk.bfc as i64;
    }
    if let Some(a) = &atk.aero {
        // p.179 Misc "Targeting Hit (per hit) +2" — one point more than the ground table's +1
        // ("Targeting critical hits may apply multiple times", fn); the base term above already
        // added +1 each, so add the aerospace difference.
        n += atk.firing_unit_targeting_crits as i64;
        n += a.kind.attack_mod() + a.target_mod() + a.misc_mod();
    }
    // Target movement + terrain — suppressed by air-to-air / ground-to-air / bombing (see
    // [`SbfAeroKind::suppresses_target_movement`]); strafe/strike keep them (Open Q 25).
    if !atk.aero.is_some_and(|a| a.kind.suppresses_target_movement()) {
        n += atk.target_tmm.max(0)
            + atk.target_jump as i64
            + atk.target_evaded as i64
            + atk.terrain;
    }
    n
}

/// Strafing damage (p.180): a successful strafing attack does one-quarter of the Flight's
/// Short-range value, rounding up (per-Flight rolls, up to 4 formations along the flight path).
pub fn sbf_strafe_damage(short: f32) -> i64 {
    (f64::from(short) / 4.0).ceil() as i64
}

/// High-Explosive bomb damage per bomb attack (p.180).
pub const SBF_BOMB_HE_DAMAGE: i64 = 2;
/// Cluster bomb damage per bomb attack (p.180) — the flip side of the −1 Cluster Bomb to-hit row.
pub const SBF_BOMB_CLUSTER_DAMAGE: i64 = 1;

/// One result on the single SBF critical-hit table (§4.2). IO:BF uses ONE table — the per-unit-type
/// columns on IO p.87 are Standard BattleForce, not SBF. No Motive/MP result exists on this table
/// (MP crits are a manual mark only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SbfCrit {
    /// 2–4: nothing.
    None,
    /// 5–7: a targeting crit (+1 to-hit, permanent).
    Targeting,
    /// 8–9: a damage crit (−1 damage at every band, floored 0).
    Damage,
    /// 10–11: both a targeting and a damage crit.
    Both,
    /// 12: the unit is destroyed.
    Destroyed,
}

/// Read the SBF crit table for a 2d6 `roll` (§4.2). Pure — rolls nothing; the player supplies the
/// total (out-of-range inputs clamp to 2..=12).
pub fn sbf_crit(roll: u8) -> SbfCrit {
    match roll.clamp(2, 12) {
        2..=4 => SbfCrit::None,
        5..=7 => SbfCrit::Targeting,
        8 | 9 => SbfCrit::Damage,
        10 | 11 => SbfCrit::Both,
        _ => SbfCrit::Destroyed, // 12
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A base ground BattleMech element; tests tweak fields via struct-update.
    fn bm() -> AsElement {
        AsElement {
            name: "Test".into(),
            as_type: "BM".into(),
            sbf_type: SbfElementType::Bm,
            size: 3,
            primary_move: 8,
            primary_mode: String::new(),
            jump_move: 0,
            skill: 4,
            full_armor: 6,
            full_structure: 4,
            std_damage: DamageVector { s: 4.0, m: 3.0, l: Some(2.0), e: Some(0.0) },
            overheat: 0,
            threshold: 0,
            suas: BTreeMap::new(),
            turret_suas: BTreeMap::new(),
            base_pv: 30,
        }
    }

    #[test]
    fn lance_of_four_hand_computed() {
        let u = convert_unit("Lance", &[bm(), bm(), bm(), bm()]);
        assert_eq!(u.sbf_type, SbfElementType::Bm); // majority round(2.667)=3, count 4 ≥ 3
        assert_eq!(u.size, 3); // mean(3)
        assert_eq!(u.movement, 4); // round(mean(8)/2) = round(4)
        assert_eq!(u.move_mode, SbfMoveMode::MekWalk);
        assert_eq!(u.jump_move, 0);
        assert_eq!(u.trsp_movement, 4); // branch 2 (no transportable) → mirrors movement
        assert_eq!(u.trsp_mode, SbfMoveMode::MekWalk);
        assert_eq!(u.tmm, 1); // tmm_from_move(4)
        assert_eq!(u.armor, 14); // per elem 6+4+0.5(str≥3)=10.5, ×4=42, /3=14
        assert_eq!(u.damage.s, 5.0); // round(16/3)=5
        assert_eq!(u.damage.m, 4.0); // round(12/3)=4
        assert_eq!(u.damage.l, Some(3.0)); // round(8/3)=round(2.667)=3
        assert_eq!(u.damage.e, None);
        assert_eq!(u.skill, 4);
        assert_eq!(u.point_value, 40); // (120/3)=40, skill 4
        assert!(u.suas.is_empty());
    }

    #[test]
    fn infantry_movement_floor_but_tmm_uses_rounded_average() {
        // Two mechs (move 20) + two CI (move 6): rounded_average = round(mean(20,20,6,6)/2)=round(6.5)=7;
        // min_inf = 6/2 = 3; movement = min(7,3) = 3. TMM keys off rounded_average (7) → 3, not 1.
        let mech = AsElement { primary_move: 20, ..bm() };
        let inf = AsElement { as_type: "CI".into(), sbf_type: SbfElementType::Ci, primary_move: 6, primary_mode: "f".into(), ..bm() };
        let u = convert_unit("Mixed", &[mech.clone(), mech, inf.clone(), inf]);
        assert_eq!(u.movement, 3);
        assert_eq!(u.tmm, tmm_from_move(7)); // = 3, uses the higher rounded-average, not `movement`
        assert_eq!(u.tmm, 3);
    }

    #[test]
    fn turret_suas_never_reach_the_unit() {
        // An element whose IF lives only in turret_suas must NOT contribute to the unit's IF.
        let mut e = bm();
        e.turret_suas.insert("IF".into(), SuaVal::Num(2.0));
        let u = convert_unit("Turret", &[e.clone(), e]);
        assert!(!u.suas.contains_key("IF")); // trap #1: turret arsenal is invisible to the converter
    }

    #[test]
    fn stl_mas_tmm_bonus_is_dead_but_flag_is_set() {
        // Every element has STL. The +2 TMM bonus reads unit SUAs (empty at the TMM step) → never
        // fires, so TMM is unchanged from the no-STL value; but STL/MAS/LMAS flags ARE set (step i).
        let e = AsElement { suas: BTreeMap::from([("STL".into(), SuaVal::Flag)]), ..bm() };
        let u = convert_unit("Stealth", &[e.clone(), e]);
        assert_eq!(u.tmm, 1); // == tmm_from_move(4); the +2 did NOT apply
        assert_eq!(u.suas.get("STL"), Some(&SuaVal::Flag));
        assert_eq!(u.suas.get("MAS"), Some(&SuaVal::Flag));
        assert_eq!(u.suas.get("LMAS"), Some(&SuaVal::Flag));
    }

    #[test]
    fn transport_branch3_and_car_sum() {
        // Two carriers (CAR4, move 12) + two transportable-cargo mechs (move 6): nonTransportable =
        // the two carriers (they lack MEC/XMEC... wait CAR makes them transport-capable) — build so
        // that exactly the two move-6 mechs are non-transportable and the carriers hold CAR.
        let carrier = AsElement { primary_move: 12, suas: BTreeMap::from([("CAR".into(), SuaVal::Num(4.0))]), ..bm() };
        let cargo = AsElement { primary_move: 6, ..bm() };
        let u = convert_unit("Convoy", &[carrier.clone(), carrier, cargo.clone(), cargo]);
        // branch 3: trsp = round(mean(primary over nonTransportable={6,6})/2) = round(3) = 3.
        assert_eq!(u.trsp_movement, 3);
        // CAR summed over the two carriers: 4 + 4 = 8.
        assert_eq!(u.suas.get("CAR"), Some(&SuaVal::Num(8.0)));
    }

    #[test]
    fn atac_double_division_reproduced() {
        // Four elements each ATAC 9. sumDivideBy3: round(36/3)=12; then /3 again: round(12/3)=4.
        let e = AsElement { suas: BTreeMap::from([("ATAC".into(), SuaVal::Num(9.0))]), ..bm() };
        let u = convert_unit("Atac", &[e.clone(), e.clone(), e.clone(), e]);
        assert_eq!(u.suas.get("ATAC"), Some(&SuaVal::Num(4.0))); // net ≈ sum/9
    }

    #[test]
    fn flk_from_top_level_ac_and_flk() {
        // Two elements each AC 2/2/2: FLK.M = round((2+2)/3)=round(1.333)=1; FLK.L = round((2+2)/3)=1.
        let e = AsElement {
            suas: BTreeMap::from([("AC".into(), SuaVal::Dmg(DamageVector { s: 2.0, m: 2.0, l: Some(2.0), e: None }))]),
            ..bm()
        };
        let u = convert_unit("Flak", &[e.clone(), e]);
        assert_eq!(
            u.suas.get("FLK"),
            Some(&SuaVal::Dmg(DamageVector { s: 0.0, m: 1.0, l: Some(1.0), e: None }))
        );
    }

    #[test]
    fn ct_folds_into_it_in_finalize() {
        // One element CT2, one IT3: sum CT=2, IT=3; finalize merges CT into IT (5) and drops CT.
        let ct = AsElement { suas: BTreeMap::from([("CT".into(), SuaVal::Num(2.0))]), ..bm() };
        let it = AsElement { suas: BTreeMap::from([("IT".into(), SuaVal::Num(3.0))]), ..bm() };
        let u = convert_unit("Cargo", &[ct, it]);
        assert_eq!(u.suas.get("IT"), Some(&SuaVal::Num(5.0)));
        assert!(!u.suas.contains_key("CT"));
    }

    #[test]
    fn formation_of_two_units_hand_computed() {
        let a = convert_unit("A", &[bm(), bm(), bm(), bm()]); // Bm, size3, move4, tmm1, skill4, pv40
        let b = convert_unit("B", &[bm(), bm()]); // Bm, size3, move4, tmm1, skill4, pv? (2 mechs)
        let f = convert_formation("Company", &[a.clone(), b.clone()]);
        assert_eq!(f.sbf_type, SbfElementType::Bm);
        assert_eq!(f.size, 3);
        assert_eq!(f.movement, 4);
        assert_eq!(f.tmm, 1);
        assert_eq!(f.skill, 4);
        assert_eq!(f.morale_rating, 7); // 3 + skill
        assert_eq!(f.point_value, a.point_value + b.point_value); // SUM, not mean
        // tactics = max(0, 10 - move(4) + skill(4) - 4) = max(0, 6) = 6.
        assert_eq!(f.tactics, 6);
        assert_eq!(f.units.len(), 2);
    }

    #[test]
    fn formation_aerospace_uses_min_movement() {
        // Two aerospace units (type As), movements 5 and 9 → formation movement = min = 5, not mean 7.
        let mk = |mv: i64| SbfUnit {
            name: "AS".into(),
            sbf_type: SbfElementType::As,
            size: 3,
            movement: mv,
            move_mode: SbfMoveMode::Aerodyne,
            jump_move: 0,
            trsp_movement: mv,
            trsp_mode: SbfMoveMode::Aerodyne,
            tmm: 2,
            armor: 5,
            damage: DamageVector { s: 3.0, m: 3.0, l: Some(2.0), e: Some(1.0) },
            skill: 4,
            point_value: 20,
            suas: BTreeMap::new(),
        };
        let f = convert_formation("Flight", &[mk(5), mk(9)]);
        assert!(f.is_aerospace());
        assert_eq!(f.movement, 5);
    }

    // ---- GOLDEN: neurohelmet convert_unit vs MegaMek's own SBFUnitConverter output ----
    // Fixtures in data/sbf-goldens/units.json are generated by running MegaMek's converters on real
    // units (regenerate via data/sbf-goldens/SbfGolden.java). We feed MegaMek's OWN dumped Alpha
    // Strike element inputs through neurohelmet's parser + convert_unit and assert the SBFUnit matches,
    // so the test is independent of neurohelmet's bake fidelity.

    use crate::domain::AsStats;
    use crate::engine::as_element::as_element;

    fn parse_type(s: &str) -> SbfElementType {
        use SbfElementType::*;
        match s {
            "BM" => Bm,
            "AS" => As,
            "MX" => Mx,
            "PM" => Pm,
            "V" => V,
            "BA" => Ba,
            "CI" => Ci,
            "MS" => Ms,
            "LA" => La,
            _ => Unknown,
        }
    }

    fn dmg_num(v: &serde_json::Value) -> f64 {
        match v.as_str().unwrap() {
            "-" => 0.0,
            "0*" => 0.5,
            x => x.parse().unwrap(),
        }
    }

    fn fmt_n(x: f32) -> String {
        if x.fract() == 0.0 {
            format!("{}", x as i64)
        } else {
            format!("{x}")
        }
    }

    /// Render one SUA into MegaMek's display form (for the SUA-set comparison).
    fn render_one(code: &str, val: &SuaVal) -> String {
        match val {
            SuaVal::Flag => code.to_string(),
            SuaVal::Num(x) => format!("{code}{}", fmt_n(*x)),
            SuaVal::Art(c) => format!("{code}{c}"),
            // FLK prints only M/L (SBFUnit.formatAbility); other vectors S/M/L.
            SuaVal::Dmg(d) if code == "FLK" => {
                format!("FLK{}/{}", d.m as i64, d.l.unwrap_or(0.0) as i64)
            }
            SuaVal::Dmg(d) => format!(
                "{code}{}/{}/{}",
                fmt_n(d.s),
                fmt_n(d.m),
                d.l.map(fmt_n).unwrap_or_else(|| "-".into())
            ),
            _ => code.to_string(),
        }
    }

    fn sorted_suas(m: &BTreeMap<String, SuaVal>) -> Vec<String> {
        let mut v: Vec<String> = m.iter().map(|(c, val)| render_one(c, val)).collect();
        v.sort();
        v
    }

    fn sorted_tokens(s: &str) -> Vec<String> {
        let mut v: Vec<String> = s.split('|').filter(|x| !x.is_empty()).map(String::from).collect();
        v.sort();
        v
    }

    fn build_elem(e: &serde_json::Value) -> AsElement {
        let s = |k: &str| e[k].as_str().unwrap().to_string();
        let i = |k: &str| e[k].as_i64().unwrap();
        let stats = AsStats {
            tp: s("tp"),
            size: i("size") as u8,
            movement: s("mv"),
            armor: i("armor") as u8,
            structure: i("structure") as u8,
            dmg_s: s("dmgS"),
            dmg_m: s("dmgM"),
            dmg_l: s("dmgL"),
            dmg_e: s("dmgE"),
            overheat: i("ov") as u8,
            threshold: i("th").max(0) as u8,
            pv: i("pv") as u16,
            specials: e["specials"]
                .as_str()
                .unwrap()
                .split('|')
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect(),
            ..Default::default()
        };
        as_element(&stats, e["name"].as_str().unwrap(), i("skill") as u8)
    }

    #[test]
    fn golden_vs_megamek_converter() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/sbf-goldens/units.json");
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(_) => {
                eprintln!("skip golden_vs_megamek_converter: {path} absent");
                return;
            }
        };
        let fixtures: serde_json::Value = serde_json::from_str(&text).unwrap();
        for fx in fixtures.as_array().unwrap() {
            let name = fx["name"].as_str().unwrap();
            let elems: Vec<AsElement> =
                fx["elements"].as_array().unwrap().iter().map(build_elem).collect();
            let u = convert_unit(name, &elems);
            let g = &fx["unit"];
            let ctx = |field: &str| format!("{name}/{field}");
            assert_eq!(u.sbf_type, parse_type(g["type"].as_str().unwrap()), "{}", ctx("type"));
            assert_eq!(u.size as i64, g["size"].as_i64().unwrap(), "{}", ctx("size"));
            assert_eq!(u.movement, g["mv"].as_i64().unwrap(), "{}", ctx("mv"));
            assert_eq!(u.move_mode.code(), g["mvCode"].as_str().unwrap(), "{}", ctx("mvCode"));
            assert_eq!(u.jump_move, g["jump"].as_i64().unwrap(), "{}", ctx("jump"));
            assert_eq!(u.trsp_movement, g["trspMv"].as_i64().unwrap(), "{}", ctx("trspMv"));
            assert_eq!(u.trsp_mode.code(), g["trspCode"].as_str().unwrap(), "{}", ctx("trspCode"));
            assert_eq!(u.tmm, g["tmm"].as_i64().unwrap(), "{}", ctx("tmm"));
            assert_eq!(u.armor, g["armor"].as_i64().unwrap(), "{}", ctx("armor"));
            assert_eq!(u.skill, g["skill"].as_i64().unwrap(), "{}", ctx("skill"));
            assert_eq!(u.point_value, g["pv"].as_i64().unwrap(), "{}", ctx("pv"));
            assert_eq!(u.damage.s as f64, dmg_num(&g["dmgS"]), "{}", ctx("dmgS"));
            assert_eq!(u.damage.m as f64, dmg_num(&g["dmgM"]), "{}", ctx("dmgM"));
            assert_eq!(u.damage.l.unwrap_or(0.0) as f64, dmg_num(&g["dmgL"]), "{}", ctx("dmgL"));
            assert_eq!(u.damage.e.unwrap_or(0.0) as f64, dmg_num(&g["dmgE"]), "{}", ctx("dmgE"));
            // MegaMek's display string hides SRCH for vehicles and SOA/SRCH for BattleMechs
            // (SBFUnit.showSUA:256-264) — a cosmetic filter, not a conversion difference. Mirror it
            // so the display-string golden compares like-for-like (the underlying aggregation of
            // those flags is still an ordinary IfHalf/IfAll path exercised elsewhere).
            let hidden: &[&str] = match u.sbf_type {
                SbfElementType::V => &["SRCH"],
                SbfElementType::Bm => &["SOA", "SRCH"],
                _ => &[],
            };
            let mut mech_suas = sorted_suas(&u.suas);
            mech_suas.retain(|s| !hidden.contains(&s.as_str()));
            assert_eq!(
                mech_suas,
                sorted_tokens(g["specials"].as_str().unwrap()),
                "{}",
                ctx("specials")
            );
        }
    }

    // ---- Phase 4: combat calculators (spec §4.1–4.2) ----

    /// Baseline hand-entered shot: skill 4, no modifiers, Medium range (+1) → 5.
    fn shot() -> SbfToHitCtx {
        SbfToHitCtx { attacker_skill: 4, ..Default::default() }
    }

    #[test]
    fn to_hit_printed_table() {
        // Range ladder — the printed IO:BF p.172 table: S +0 / M +1 / L +2 / E +3, all legal.
        assert_eq!(sbf_to_hit(&SbfToHitCtx { range: SbfRange::Short, ..shot() }), 4);
        assert_eq!(sbf_to_hit(&shot()), 5);
        assert_eq!(sbf_to_hit(&SbfToHitCtx { range: SbfRange::Long, ..shot() }), 6);
        assert_eq!(sbf_to_hit(&SbfToHitCtx { range: SbfRange::Extreme, ..shot() }), 7);
        // Indirect fire adds +1 on top of the range modifier.
        assert_eq!(
            sbf_to_hit(&SbfToHitCtx { range: SbfRange::Long, indirect_fire: true, ..shot() }),
            7
        );
        // Positive TMM adds; negative TMM is ignored.
        assert_eq!(sbf_to_hit(&SbfToHitCtx { target_tmm: 2, ..shot() }), 7);
        assert_eq!(sbf_to_hit(&SbfToHitCtx { target_tmm: -1, ..shot() }), 5);
        // Jump is +1 PER POINT used, both sides (not a flat +1).
        assert_eq!(sbf_to_hit(&SbfToHitCtx { attacker_jump: 3, ..shot() }), 8);
        assert_eq!(sbf_to_hit(&SbfToHitCtx { target_jump: 2, ..shot() }), 7);
        // Withholding fire: −1 per unit, floored at −2.
        assert_eq!(sbf_to_hit(&SbfToHitCtx { withheld_units: 1, ..shot() }), 4);
        assert_eq!(sbf_to_hit(&SbfToHitCtx { withheld_units: 3, ..shot() }), 3);
        // One-point attacker specials: BFC, drone, spotting, secondary target; target evaded.
        assert_eq!(sbf_to_hit(&SbfToHitCtx { bfc: true, ..shot() }), 6);
        assert_eq!(sbf_to_hit(&SbfToHitCtx { drone: true, ..shot() }), 6);
        assert_eq!(sbf_to_hit(&SbfToHitCtx { spotting: true, ..shot() }), 6);
        assert_eq!(sbf_to_hit(&SbfToHitCtx { secondary: true, ..shot() }), 6);
        assert_eq!(sbf_to_hit(&SbfToHitCtx { target_evaded: true, ..shot() }), 6);
        // Targeting crits: +1 per crit; terrain: hand-entered.
        assert_eq!(sbf_to_hit(&SbfToHitCtx { firing_unit_targeting_crits: 2, ..shot() }), 7);
        assert_eq!(sbf_to_hit(&SbfToHitCtx { terrain: 2, ..shot() }), 7);
        // All legs at once: 3 +1crit +0S +1IF +2jump −2withheld +1bfc +1spot +1sec +3tmm +2tjump
        // +1evade +1terrain = 15.
        assert_eq!(
            sbf_to_hit(&SbfToHitCtx {
                attacker_skill: 3,
                firing_unit_targeting_crits: 1,
                range: SbfRange::Short,
                indirect_fire: true,
                attacker_jump: 2,
                withheld_units: 5, // floors at −2
                bfc: true,
                drone: false,
                spotting: true,
                secondary: true,
                target_tmm: 3,
                target_jump: 2,
                target_evaded: true,
                terrain: 1,
                aero: None,
            }),
            15
        );
        // Morale is manual (§4.3): SbfToHitCtx has no morale field, so the number cannot depend on
        // the target's rung by construction — this comment is the spec's "morale invariance" test.
    }

    // ---- Strategic Aerospace: the p.179 table (SAS layer) ----

    /// A bare aero leg; tests tweak fields via struct-update. Attacker airborne aero (the common
    /// air-to-air case), nothing else set.
    fn aero(kind: SbfAeroKind) -> SbfAeroShot {
        SbfAeroShot {
            kind,
            target: SbfAeroTarget::AirborneAero,
            attacker_airborne_aero: true,
            behind_target: false,
            grounded_dropship: false,
            sv_fire_control: SbfSvFireControl::Afc,
        }
    }

    /// Baseline air-to-air shot: skill 4 + Medium (+1) = 5; the +2 airborne row is gated off
    /// (attacker is itself an airborne aerospace Squadron, p.179 fn).
    fn a2a() -> SbfToHitCtx {
        SbfToHitCtx { aero: Some(aero(SbfAeroKind::AirToAir)), ..shot() }
    }

    #[test]
    fn aero_to_hit_target_rows() {
        let vs = |target| SbfToHitCtx {
            aero: Some(SbfAeroShot { target, ..aero(SbfAeroKind::AirToAir) }),
            ..shot()
        };
        // Airborne aerospace +2 is gated on the attacker NOT being airborne aerospace (fn) —
        // a true air-to-air shot never takes it.
        assert_eq!(sbf_to_hit(&a2a()), 5);
        let ground_attacker = |target| SbfToHitCtx {
            aero: Some(SbfAeroShot {
                target,
                attacker_airborne_aero: false,
                ..aero(SbfAeroKind::GroundToAir)
            }),
            ..shot()
        };
        assert_eq!(sbf_to_hit(&ground_attacker(SbfAeroTarget::AirborneAero)), 7);
        // The DropShip −2 / Small Craft −1 rows stack on the gated +2 (both are airborne
        // aerospace per the fn): ground-to-air vs a DropShip is "effectively a +0" (p.181).
        assert_eq!(sbf_to_hit(&ground_attacker(SbfAeroTarget::AirborneDropship)), 5);
        assert_eq!(sbf_to_hit(&vs(SbfAeroTarget::AirborneDropship)), 3);
        assert_eq!(sbf_to_hit(&ground_attacker(SbfAeroTarget::SmallCraft)), 6);
        assert_eq!(sbf_to_hit(&vs(SbfAeroTarget::SmallCraft)), 4);
        // VTOL/WiGE +1 is not an aerospace row — no gated +2 underneath, either attacker.
        assert_eq!(sbf_to_hit(&ground_attacker(SbfAeroTarget::AirborneVtolWige)), 6);
        assert_eq!(sbf_to_hit(&vs(SbfAeroTarget::AirborneVtolWige)), 6);
        // Grounded Squadron: the −4 immobile target row (p.181).
        assert_eq!(sbf_to_hit(&ground_attacker(SbfAeroTarget::GroundedSquadron)), 1);
        // A ground Formation has no target-type row.
        assert_eq!(sbf_to_hit(&vs(SbfAeroTarget::GroundFormation)), 5);
    }

    #[test]
    fn aero_to_hit_a2g_rows() {
        let a2g = |a| SbfToHitCtx {
            aero: Some(SbfAeroShot {
                target: SbfAeroTarget::GroundFormation,
                ..aero(SbfAeroKind::A2G(a))
            }),
            ..shot()
        };
        // Altitude +3 / Dive +2 / Strafing +4 (vs Standard BF's +2 — different scale) / Strike +2.
        assert_eq!(sbf_to_hit(&a2g(SbfA2G::AltitudeBombing { cluster: false })), 8);
        assert_eq!(sbf_to_hit(&a2g(SbfA2G::DiveBombing { cluster: false })), 7);
        assert_eq!(sbf_to_hit(&a2g(SbfA2G::Strafing)), 9);
        assert_eq!(sbf_to_hit(&a2g(SbfA2G::Striking)), 7);
        // Cluster Bomb −1 rides on a bombing attack.
        assert_eq!(sbf_to_hit(&a2g(SbfA2G::AltitudeBombing { cluster: true })), 7);
        assert_eq!(sbf_to_hit(&a2g(SbfA2G::DiveBombing { cluster: true })), 6);
    }

    #[test]
    fn aero_to_hit_misc_rows() {
        // Behind the target −2; grounded DropShip −2.
        let behind = SbfToHitCtx {
            aero: Some(SbfAeroShot { behind_target: true, ..aero(SbfAeroKind::AirToAir) }),
            ..shot()
        };
        assert_eq!(sbf_to_hit(&behind), 3);
        let grounded_ds = SbfToHitCtx {
            aero: Some(SbfAeroShot { grounded_dropship: true, ..aero(SbfAeroKind::AirToAir) }),
            ..shot()
        };
        assert_eq!(sbf_to_hit(&grounded_ds), 3);
        // SV fire control: AFC +0 / BFC +1 / neither +2.
        let sv = |fc| SbfToHitCtx {
            aero: Some(SbfAeroShot { sv_fire_control: fc, ..aero(SbfAeroKind::AirToAir) }),
            ..shot()
        };
        assert_eq!(sbf_to_hit(&sv(SbfSvFireControl::Afc)), 5);
        assert_eq!(sbf_to_hit(&sv(SbfSvFireControl::Bfc)), 6);
        assert_eq!(sbf_to_hit(&sv(SbfSvFireControl::None)), 7);
        // The ground `bfc` flag is superseded by the SV ladder under an aero shot (never both) …
        assert_eq!(sbf_to_hit(&SbfToHitCtx { bfc: true, ..sv(SbfSvFireControl::Bfc) }), 6);
        assert_eq!(sbf_to_hit(&SbfToHitCtx { bfc: true, ..sv(SbfSvFireControl::Afc) }), 5);
        // … while the drone +1 is the same row on both tables — applied once.
        assert_eq!(sbf_to_hit(&SbfToHitCtx { drone: true, ..a2a() }), 6);
        // Targeting crits are +2 each on the p.179 table (fn: "may apply multiple times") vs the
        // ground table's +1 each.
        assert_eq!(
            sbf_to_hit(&SbfToHitCtx { firing_unit_targeting_crits: 2, ..a2a() }),
            9,
            "+2 per targeting crit under an aero shot"
        );
        assert_eq!(
            sbf_to_hit(&SbfToHitCtx { firing_unit_targeting_crits: 2, ..shot() }),
            7,
            "+1 per targeting crit on a ground shot"
        );
    }

    #[test]
    fn aero_suppresses_target_movement_by_kind() {
        // A full hand of target-movement/terrain legs: +3 TMM +2 jump +1 evaded +2 terrain = +8.
        let legs = |kind| SbfToHitCtx {
            target_tmm: 3,
            target_jump: 2,
            target_evaded: true,
            terrain: 2,
            aero: Some(SbfAeroShot {
                target: SbfAeroTarget::GroundFormation,
                ..aero(kind)
            }),
            ..shot()
        };
        // Air-to-air and ground-to-air: no target movement or terrain modifiers (p.179/p.181).
        assert_eq!(sbf_to_hit(&legs(SbfAeroKind::AirToAir)), 5);
        assert_eq!(sbf_to_hit(&legs(SbfAeroKind::GroundToAir)), 5);
        // Bombing skips the target's movement/type/terrain (p.180 Step 3).
        assert_eq!(sbf_to_hit(&legs(SbfAeroKind::A2G(SbfA2G::AltitudeBombing { cluster: false }))), 8);
        assert_eq!(sbf_to_hit(&legs(SbfAeroKind::A2G(SbfA2G::DiveBombing { cluster: false }))), 7);
        // Strafe/strike keep them (Open Q 25, DECIDED — Step 3 over the Targeting paragraph).
        assert_eq!(sbf_to_hit(&legs(SbfAeroKind::A2G(SbfA2G::Strafing))), 5 + 4 + 8);
        assert_eq!(sbf_to_hit(&legs(SbfAeroKind::A2G(SbfA2G::Striking))), 5 + 2 + 8);
        // A ground shot (no aero leg) is untouched by any of this.
        assert_eq!(sbf_to_hit(&SbfToHitCtx { aero: None, ..legs(SbfAeroKind::AirToAir) }), 13);
    }

    #[test]
    fn strafe_and_bomb_damage() {
        // Strafing: ¼ of the Flight's Short value, round up (p.180).
        assert_eq!(sbf_strafe_damage(5.0), 2); // 1.25 → 2
        assert_eq!(sbf_strafe_damage(2.0), 1); // 0.5 → 1
        assert_eq!(sbf_strafe_damage(4.0), 1); // exact quarter stays
        assert_eq!(sbf_strafe_damage(8.0), 2);
        assert_eq!(sbf_strafe_damage(0.5), 1); // the 0* minimal band still lands a point
        assert_eq!(sbf_strafe_damage(0.0), 0);
        // Bombs: HE 2 / Cluster 1 per bomb attack (p.180).
        assert_eq!(SBF_BOMB_HE_DAMAGE, 2);
        assert_eq!(SBF_BOMB_CLUSTER_DAMAGE, 1);
    }

    #[test]
    fn crit_table() {
        // The single SBF table (§4.2): 2-4 none / 5-7 targeting / 8-9 damage / 10-11 both / 12 kill.
        for (roll, want) in [
            (2, SbfCrit::None),
            (3, SbfCrit::None),
            (4, SbfCrit::None),
            (5, SbfCrit::Targeting),
            (6, SbfCrit::Targeting),
            (7, SbfCrit::Targeting),
            (8, SbfCrit::Damage),
            (9, SbfCrit::Damage),
            (10, SbfCrit::Both),
            (11, SbfCrit::Both),
            (12, SbfCrit::Destroyed),
        ] {
            assert_eq!(sbf_crit(roll), want, "roll {roll}");
        }
        // Out-of-range totals clamp to the table edges.
        assert_eq!(sbf_crit(0), SbfCrit::None);
        assert_eq!(sbf_crit(13), SbfCrit::Destroyed);
    }

    #[test]
    fn band_selection_and_crit_floor() {
        let ground = DamageVector { s: 5.0, m: 4.0, l: Some(2.0), e: None };
        assert_eq!(ground.band(SbfRange::Short), 5.0);
        assert_eq!(ground.band(SbfRange::Medium), 4.0);
        assert_eq!(ground.band(SbfRange::Long), 2.0);
        // No E band → Extreme deals L − 1 (Step 5a), floored at 0.
        assert_eq!(ground.band(SbfRange::Extreme), 1.0);
        assert_eq!(
            DamageVector { s: 1.0, m: 1.0, l: Some(0.0), e: None }.band(SbfRange::Extreme),
            0.0
        );
        // An explicit E band (aerospace) wins over the L−1 fallback.
        assert_eq!(
            DamageVector { s: 3.0, m: 3.0, l: Some(2.0), e: Some(1.0) }.band(SbfRange::Extreme),
            1.0
        );
        // Damage crits floor every band at 0 — never negative (§4.2 crit floors).
        let hit3 = reduced_by(ground, 3);
        assert_eq!(hit3, DamageVector { s: 2.0, m: 1.0, l: Some(0.0), e: None });
        assert_eq!(reduced_by(ground, 9), DamageVector { s: 0.0, m: 0.0, l: Some(0.0), e: None });
    }
}
