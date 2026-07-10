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

//! Phase 1 of Strategic BattleForce (SBF) support (see `docs/sbf-implementation-spec.md`): a typed
//! view of one Alpha Strike element parsed from a unit's baked [`AsStats`]. neurohelmet stores AS stats
//! as the verbatim Mekbay display strings (damage cells, a movement string, and a list of
//! special-ability tokens); this module parses them into numbers, a damage vector, and a typed SUA
//! map so the SBF converters (Phase 2) can aggregate elements into Units and Formations.
//!
//! Pure functions of `AsStats` + a skill byte; no IO, exhaustively unit-tested.
//!
//! **Turret parity (Data-fidelity trap #1).** MegaMek's `SBFUnitConverter` reads an element's SUAs
//! only through the top-level ability map (`AlphaStrikeElement.hasSUA` → `ASSpecialAbilityCollection`
//! `containsKey`), so turret-embedded abilities (`TUR(…)`) are invisible to it. To keep neurohelmet's
//! converter bug-for-bug faithful (the golden invariant), `TUR(…)` interior SUAs parse into a
//! SEPARATE [`AsElement::turret_suas`] map that the query helpers and the converter never read. Do
//! not union turret contents into `suas`.

use crate::domain::AsStats;
use std::collections::BTreeMap;

/// A bracketed damage vector (2–4 bands). A missing band or a printed `-` is `None`; `0*`
/// (minimal) is `0.5`. `s`/`m` are always present (default `0.0`); `l`/`e` are optional.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DamageVector {
    pub s: f32,
    pub m: f32,
    pub l: Option<f32>,
    pub e: Option<f32>,
}

/// SBF element type (`SBFElementType.java:28`). Ordering is not meaningful; it is a plain enum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SbfElementType {
    #[default]
    Unknown,
    Bm,
    As,
    Mx,
    Pm,
    V,
    Ba,
    Ci,
    Ms,
    La,
}

impl SbfElementType {
    /// `SBFElementType.java:63-65`.
    pub fn is_ground(self) -> bool {
        matches!(
            self,
            Self::Bm | Self::Mx | Self::Pm | Self::V | Self::Ba | Self::Ci | Self::Ms
        )
    }

    /// BROAD aerospace test (`SBFElementType.java:68-70`) — `Unknown`, `As`, `La` count as
    /// aerospace. This mirrors `SBFUnit.isAerospace` and is the predicate the UNIT-movement
    /// converter uses (Phase 2 §2.3). It is DISTINCT from the narrow formation test (§2.4) — do
    /// not reuse this one for formation movement, or you will pick min-thrust for `Unknown`
    /// formations where MegaMek uses the mean.
    pub fn is_aerospace(self) -> bool {
        !self.is_ground()
    }
}

/// One resolved SUA value. `TUR(…)` contents are parsed into a SEPARATE turret map, NEVER into an
/// element's home `suas` map, to preserve MegaMek converter parity (Data-fidelity trap #1).
#[derive(Clone, Debug, PartialEq)]
pub enum SuaVal {
    /// Presence only: `CASE`, `ENE`, `STL`, `OMNI`, `RCN`, `AMS`, `CR`, `RAMS`, `OVL`, `SOA`, …
    Flag,
    /// A scalar value: `IF1`, `IF0*`(=0.5), `CAR4`, `IT1.95`, `CT0.009`, `MHQ0`, `BOMB2`, `FUEL20`, …
    Num(f32),
    /// Artillery count, e.g. `ARTAIS-2` → `Art(2)`.
    Art(u8),
    /// A damage-vector ability: `AC`, `FLK`, `HT`, `IATM`, `LRM`, `SRM`, `TOR`, `REAR`.
    Dmg(DamageVector),
    /// `LAM(6"g/12a)` → ground/WiGE + aerospace thrust legs.
    Lam { wige: u32, thrust: u32 },
    /// `BIM(…)` → the aerospace thrust leg.
    Bim(u32),
}

/// Everything the SBF converters read off one AS element.
#[derive(Clone, Debug, Default)]
pub struct AsElement {
    /// For `isConsideredRcn` name tests (Phase 2).
    pub name: String,
    /// Raw AS type code (`"BM"`, `"CV"`, …); the string `mode_for_element` matches its type
    /// qualifiers against (Phase 2 §2.2), and the source for the `isAerospaceSV` routing note.
    pub as_type: String,
    /// `getUnitType()` result (mapping in [`sbf_type_from_tp`]).
    pub sbf_type: SbfElementType,
    pub size: u8,
    /// Inches; the first movement token's value.
    pub primary_move: u32,
    /// The first token's mode suffix (`""`, `"j"`, `"t"`, `"w"`, `"h"`, `"v"`, `"s"`, `"g"`, `"a"`, …).
    pub primary_mode: String,
    pub jump_move: u32,
    /// `TrackedMech.gunnery`; default 4.
    pub skill: u8,
    pub full_armor: u16,
    pub full_structure: u16,
    /// Front (standard) S/M/L/E damage from `dmg_s..dmg_e`.
    pub std_damage: DamageVector,
    pub overheat: u8,
    pub threshold: u8,
    /// TOP-LEVEL SUAs only — the ONLY map the converter reads.
    pub suas: BTreeMap<String, SuaVal>,
    /// `TUR(…)` interior SUAs; parsed for fidelity/UI, IGNORED by `convert_unit`.
    pub turret_suas: BTreeMap<String, SuaVal>,
    /// `AsStats.pv` (the skill-4 base PV).
    pub base_pv: u16,
    /// Large-craft multi-arc card (DropShip→WarShip), if any — the four firing arcs × weapon
    /// classes. `None` for single-arc units (fighters, ground). See [`super::large_craft`].
    pub arcs: Option<crate::domain::ArcCard>,
}

impl AsElement {
    /// Top-level presence only, mirroring `AlphaStrikeElement.hasSUA`. Turret SUAs are deliberately
    /// NOT visible here (trap #1).
    pub fn has_sua(&self, code: &str) -> bool {
        self.suas.contains_key(code)
    }

    pub fn has_any_sua(&self, codes: &[&str]) -> bool {
        codes.iter().any(|c| self.has_sua(c))
    }

    /// `getSUA`-as-number, mirroring MegaMek's numeric getters (`getCAR`/`getIT`/`getMHQ`/…): the
    /// stored numeric value, or 0 when the code is absent. `Num(v)→v`, `Art(c)→c`, `Dmg(d)→d.s`.
    ///
    /// The `Flag→1.0` case is NOT a general MegaMek behavior — it exists only so the Phase-2
    /// aggregation merge helper can mirror MegaMek's `null value → +1` merge rule. Do not rely on it
    /// in a general numeric read; an absent code returns `0.0`, exactly as MegaMek treats a missing
    /// value.
    pub fn sua_num(&self, code: &str) -> f32 {
        match self.suas.get(code) {
            Some(SuaVal::Num(v)) => *v,
            Some(SuaVal::Art(c)) => *c as f32,
            Some(SuaVal::Dmg(d)) => d.s,
            Some(SuaVal::Flag) => 1.0,
            _ => 0.0,
        }
    }

    /// The damage vector iff this code is a top-level `Dmg` SUA.
    pub fn sua_dmg(&self, code: &str) -> Option<DamageVector> {
        match self.suas.get(code) {
            Some(SuaVal::Dmg(d)) => Some(*d),
            _ => None,
        }
    }

    pub fn get_ov(&self) -> f32 {
        self.overheat as f32
    }

    /// `fuelRating` (`SBFUnitConverter.java:260-266`): the `FUEL` value; if this is a fighter
    /// (`AF`/`CF`) and its fuel is 0, it is treated as 4.
    pub fn fuel_rating(&self) -> f32 {
        let fuel = self.sua_num("FUEL");
        if (self.as_type == "AF" || self.as_type == "CF") && fuel == 0.0 {
            4.0
        } else {
            fuel
        }
    }
}

/// `getUnitType` mapping (`SBFElementType.java:31-55`) — build `sbf_type` from the AS `tp` code
/// alone. A bare `SV` routes to `V`; [`as_element`] refines an *aerodyne* `SV` (a fixed-wing
/// Support Vehicle) up to `As` via the movement mode (`isAerospaceSV()`). Unbaked large-craft/PM/MS
/// types fall to the `default → La` arm; neurohelmet-local `BD` (gun emplacement) → `V`.
pub fn sbf_type_from_tp(tp: &str) -> SbfElementType {
    match tp {
        "IM" | "BM" => SbfElementType::Bm,
        "PM" => SbfElementType::Pm,
        "MS" => SbfElementType::Ms,
        "BA" => SbfElementType::Ba,
        "CI" => SbfElementType::Ci,
        "AF" | "CF" | "SC" => SbfElementType::As,
        "CV" => SbfElementType::V,
        "SV" => SbfElementType::V, // ground SV; aerodyne (fixed-wing) SVs → As in as_element()
        "BD" => SbfElementType::V, // neurohelmet-local gun emplacement
        _ => SbfElementType::La,
    }
}

/// Build the typed element from a unit's baked AS stats, a display name, and a skill (gunnery).
pub fn as_element(stats: &AsStats, name: &str, skill: u8) -> AsElement {
    let (primary_move, jump_move, primary_mode) = parse_movement(&stats.movement);
    let std_damage = DamageVector {
        s: as_damage(&stats.dmg_s),
        m: as_damage(&stats.dmg_m),
        l: Some(as_damage(&stats.dmg_l)),
        e: Some(as_damage(&stats.dmg_e)),
    };
    let mut suas = BTreeMap::new();
    let mut turret_suas = BTreeMap::new();
    for tok in &stats.specials {
        parse_special(tok, &mut suas, &mut turret_suas);
    }
    // isAerospaceSV() (SBFElementType.java): the tp code "SV" can't distinguish an aerospace
    // (fixed-wing) Support Vehicle from a ground one, but the AS movement mode can — aerodyne (`a`)
    // is aerospace-only in the catalog (ground SVs use h/t/w/v/…; airship SVs bake as ground). Route
    // aerodyne SVs up to `As`; everything else keeps the tp-only mapping.
    let sbf_type = if stats.tp == "SV" && primary_mode == "a" {
        SbfElementType::As
    } else {
        sbf_type_from_tp(&stats.tp)
    };
    AsElement {
        name: name.to_string(),
        as_type: stats.tp.clone(),
        sbf_type,
        size: stats.size,
        primary_move,
        primary_mode,
        jump_move,
        skill,
        full_armor: stats.armor as u16,
        full_structure: stats.structure as u16,
        std_damage,
        overheat: stats.overheat,
        threshold: stats.threshold,
        suas,
        turret_suas,
        base_pv: stats.pv,
        arcs: stats.arcs.clone(),
    }
}

/// A single main damage cell: `"0*"`→0.5, `"-"`→0.0, otherwise the integer (`ASDamage.java:151-153`).
pub(crate) fn as_damage(s: &str) -> f32 {
    match s.trim() {
        "0*" => 0.5,
        "-" => 0.0,
        x if x.ends_with('*') => 0.5, // only `0*` occurs catalog-wide, but be defensive
        x => x.parse().unwrap_or(0.0),
    }
}

/// A single bracket inside a damage vector: `"-"`→None, `"0*"`→Some(0.5), int→Some.
fn parse_bracket(s: &str) -> Option<f32> {
    match s.trim() {
        "-" => None,
        "0*" => Some(0.5),
        x if x.ends_with('*') => Some(0.5),
        x => x.parse().ok(),
    }
}

/// Parse a `/`-joined damage vector (`"2/2/-"`, `"1/1/1/-"`, `"2/2"`). Bands beyond what is present
/// are `None` for `l`/`e` and `0.0` for the mandatory `s`/`m`.
fn parse_damage_vector(s: &str) -> DamageVector {
    let p: Vec<&str> = s.split('/').collect();
    DamageVector {
        s: p.first().and_then(|x| parse_bracket(x)).unwrap_or(0.0),
        m: p.get(1).and_then(|x| parse_bracket(x)).unwrap_or(0.0),
        l: p.get(2).and_then(|x| parse_bracket(x)),
        e: p.get(3).and_then(|x| parse_bracket(x)),
    }
}

/// Parse a movement string into `(primary_inches, jump_inches, primary_mode_suffix)`. Tokens are
/// `/`-joined; a ground token is `<int>"<mode?>` (has the literal `"`), an aero token is `<int>a`.
/// `primary` is the first token's value and `mode` its suffix; `jump` is the value of the token
/// whose suffix is exactly `j` (0 if none). A lone `6"j` is jump-only → 6 is both primary and jump.
fn parse_movement(mv: &str) -> (u32, u32, String) {
    let mut primary = 0u32;
    let mut primary_mode = String::new();
    let mut jump = 0u32;
    for (i, raw) in mv.split('/').enumerate() {
        let (num, suffix) = parse_move_token(raw.trim());
        if i == 0 {
            primary = num;
            primary_mode = suffix.clone();
        }
        if suffix == "j" {
            jump = num;
        }
    }
    (primary, jump, primary_mode)
}

/// Split one movement token into `(value, mode_suffix)`: leading digits are the value; a leading
/// `"` after the digits is dropped; the remainder is the mode suffix (`""`, `"j"`, `"a"`, …).
fn parse_move_token(tok: &str) -> (u32, String) {
    // Station-keeping large craft serialize as `0.<value>k` (MegaMek `AlphaStrikeHelper.moveString`):
    // the value is the digits AFTER `0.`, not a leading zero. Without this, `0.2k` → (0, ".2k").
    if let Some(rest) = tok.strip_prefix("0.") {
        let n = rest.bytes().take_while(u8::is_ascii_digit).count();
        return (rest[..n].parse().unwrap_or(0), rest[n..].to_string());
    }
    let n = tok.bytes().take_while(u8::is_ascii_digit).count();
    let num = tok[..n].parse().unwrap_or(0);
    let rest = &tok[n..];
    let suffix = rest.strip_prefix('"').unwrap_or(rest);
    (num, suffix.to_string())
}

/// Route one special-ability token into the flat top-level map `out` or, for `TUR(…)` interior
/// tokens, the `turret` map (the deliberate non-union that keeps the converter MegaMek-faithful,
/// trap #1). See `docs/sbf-implementation-spec.md` §"Exact parser rules".
fn parse_special(tok: &str, out: &mut BTreeMap<String, SuaVal>, turret: &mut BTreeMap<String, SuaVal>) {
    let tok = tok.trim();
    // TUR(bare-vector, sua, sua, …): store the bare front vector, route the rest INTO `turret`.
    if let Some(inner) = tok.strip_prefix("TUR(").and_then(|s| s.strip_suffix(')')) {
        for part in inner.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if leading_alpha(part).is_empty() && part.contains('/') {
                turret.insert("TUR".to_string(), SuaVal::Dmg(parse_damage_vector(part)));
            } else {
                // Interior SUAs land in the turret map, never in the flat top-level set (trap #1).
                insert_special(part, turret);
            }
        }
        return;
    }
    insert_special(tok, out);
}

/// Classify one non-`TUR` token and insert it into a single map. Used for both the flat top-level
/// map and (for turret interiors) the turret map.
fn insert_special(tok: &str, out: &mut BTreeMap<String, SuaVal>) {
    let tok = tok.trim();
    if tok.is_empty() {
        return;
    }
    // neurohelmet-local tokens with no standard SBF meaning: whitelist-skip rather than choke.
    if tok.contains(':') || tok == "IMMOBILE" {
        return;
    }
    if let Some(inner) = tok.strip_prefix("LAM(").and_then(|s| s.strip_suffix(')')) {
        let mut it = inner.split('/');
        let wige = it.next().map(leading_number).unwrap_or(0);
        let thrust = it.next().map(leading_number).unwrap_or(0);
        out.insert("LAM".to_string(), SuaVal::Lam { wige, thrust });
        return;
    }
    if let Some(inner) = tok.strip_prefix("BIM(").and_then(|s| s.strip_suffix(')')) {
        let thrust = inner.split('/').next().map(leading_number).unwrap_or(0);
        out.insert("BIM".to_string(), SuaVal::Bim(thrust));
        return;
    }
    // Artillery: `ART<subtype>-<count>` (split on the LAST '-', since the subtype may embed digits).
    if tok.starts_with("ART") {
        let (code, count) = match tok.rfind('-') {
            Some(i) => (&tok[..i], tok[i + 1..].parse().unwrap_or(0)),
            None => (tok, 0u8),
        };
        out.insert(code.to_string(), SuaVal::Art(count));
        return;
    }
    // Damage-vector abilities (`AC2/2/-`, `REAR1/1/-/-`, `FLK0*/1/1`): code = leading letters.
    if tok.contains('/') {
        let code = leading_alpha(tok);
        out.insert(
            code.to_string(),
            SuaVal::Dmg(parse_damage_vector(&tok[code.len()..])),
        );
        return;
    }
    // Transport with a door suffix: `<CODE><value>-D<doors>` → keep the numeric transport value.
    if let Some(i) = tok.rfind("-D") {
        let doors = &tok[i + 2..];
        if !doors.is_empty() && doors.bytes().all(|b| b.is_ascii_digit()) {
            if let Some((code, num)) = split_trailing_number(&tok[..i]) {
                out.insert(code.to_string(), SuaVal::Num(as_damage(num)));
                return;
            }
        }
    }
    // Hyphenated tokens: value-bearing ones (`TSEMP-O1` → code `TSEMP-O`, value 1) keep their
    // value; the rest (`I-TSM`, …) are presence flags matched whole. Transport `-D` and `ART-` are
    // already handled above.
    if tok.contains('-') {
        match split_trailing_number(tok) {
            Some((code, num)) => out.insert(code.to_string(), SuaVal::Num(as_damage(num))),
            None => out.insert(tok.to_string(), SuaVal::Flag),
        };
        return;
    }
    // Presence-only SUAs that END IN A DIGIT — MegaMek keeps these whole (`hasSUA(NC3)` etc.), so
    // they must NOT be split into code+value. These three are the only such codes catalog-wide.
    if matches!(tok, "NC3" | "BHJ2" | "BHJ3") {
        out.insert(tok.to_string(), SuaVal::Flag);
        return;
    }
    // Trailing numeric run (incl. decimals and `0*`) → a scalar value; else presence-only flag.
    // C3 masters (`C3M2`, `C3BSM2`) land here: the trailing value is stripped but the INTERNAL digit
    // is preserved, so `has_sua("C3M")` matches MegaMek (whose enum key is `C3M`, value 2).
    if let Some((code, num)) = split_trailing_number(tok) {
        out.insert(code.to_string(), SuaVal::Num(as_damage(num)));
        return;
    }
    out.insert(tok.to_string(), SuaVal::Flag);
}

/// The leading run of ASCII letters (the SUA code prefix).
fn leading_alpha(s: &str) -> &str {
    let n = s.bytes().take_while(u8::is_ascii_alphabetic).count();
    &s[..n]
}

/// The leading run of digits as a number (0 if none).
fn leading_number(s: &str) -> u32 {
    let n = s.bytes().take_while(u8::is_ascii_digit).count();
    s[..n].parse().unwrap_or(0)
}

/// Split a token into `(code, numeric_suffix)` where the suffix is the trailing run of digits, `.`,
/// and a possible `*`. Returns `None` if there is no trailing digit or no code prefix.
fn split_trailing_number(tok: &str) -> Option<(&str, &str)> {
    let b = tok.as_bytes();
    let mut j = b.len();
    while j > 0 && (b[j - 1].is_ascii_digit() || b[j - 1] == b'.' || b[j - 1] == b'*') {
        j -= 1;
    }
    let num = &tok[j..];
    if j == 0 || !num.bytes().any(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((&tok[..j], num))
}

/// Java `Math.round(double)` = `floor(x + 0.5)`: round half UP toward +∞. This differs from Rust's
/// `f64::round` (half AWAY from zero) for negative halves — used by all SBF phases.
#[inline]
pub fn jround(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an `AsStats` for tests. Damage cells default to "0"; caller overrides what matters.
    fn stats(tp: &str, movement: &str, specials: &[&str]) -> AsStats {
        AsStats {
            tp: tp.to_string(),
            movement: movement.to_string(),
            dmg_s: "0".into(),
            dmg_m: "0".into(),
            dmg_l: "0".into(),
            dmg_e: "0".into(),
            specials: specials.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn damage_cell_forms() {
        assert_eq!(as_damage("0*"), 0.5);
        assert_eq!(as_damage("0"), 0.0);
        assert_eq!(as_damage("7"), 7.0);
        assert_eq!(as_damage("-"), 0.0);
    }

    #[test]
    fn movement_primary_jump_mode() {
        assert_eq!(parse_movement("10\"/8\"j"), (10, 8, String::new()));
        assert_eq!(parse_movement("6\"j"), (6, 6, "j".to_string())); // jump-only: 6 is both
        assert_eq!(parse_movement("7a"), (7, 0, "a".to_string())); // aero thrust, no quote
        assert_eq!(parse_movement("0\"t"), (0, 0, "t".to_string()));
        // Real multi-token shapes from the catalog:
        assert_eq!(parse_movement("8\"/6\"s"), (8, 0, String::new())); // secondary submarine, no jump
        assert_eq!(parse_movement("5\"h/4\"j"), (5, 4, "h".to_string())); // hover primary + jump
    }

    #[test]
    fn damage_vector_bands() {
        assert_eq!(
            parse_damage_vector("2/2/-"),
            DamageVector { s: 2.0, m: 2.0, l: None, e: None }
        );
        // REAR 4-band with trailing dashes:
        assert_eq!(
            parse_damage_vector("1/1/-/-"),
            DamageVector { s: 1.0, m: 1.0, l: None, e: None }
        );
        // Minimal bracket inside a vector:
        assert_eq!(
            parse_damage_vector("0*/1/1"),
            DamageVector { s: 0.5, m: 1.0, l: Some(1.0), e: None }
        );
    }

    #[test]
    fn atlas_top_level_specials() {
        // Atlas AS7-D: 5/5/2/0 front damage; AC/IF/LRM/REAR carried at TOP LEVEL.
        let mut s = stats("BM", "6\"", &["AC2/2/-", "IF1", "LRM1/1/1", "REAR1/1/-"]);
        s.dmg_s = "5".into();
        s.dmg_m = "5".into();
        s.dmg_l = "2".into();
        s.dmg_e = "0".into();
        let e = as_element(&s, "Atlas AS7-D", 4);
        assert_eq!(e.sbf_type, SbfElementType::Bm);
        assert_eq!(
            e.std_damage,
            DamageVector { s: 5.0, m: 5.0, l: Some(2.0), e: Some(0.0) }
        );
        assert_eq!(e.sua_dmg("AC"), Some(DamageVector { s: 2.0, m: 2.0, l: None, e: None }));
        assert_eq!(e.sua_num("IF"), 1.0);
        assert_eq!(e.sua_dmg("LRM"), Some(DamageVector { s: 1.0, m: 1.0, l: Some(1.0), e: None }));
        assert_eq!(e.sua_dmg("REAR"), Some(DamageVector { s: 1.0, m: 1.0, l: None, e: None }));
        assert!(e.turret_suas.is_empty());
    }

    #[test]
    fn turret_suas_are_not_unioned() {
        // TUR(front-vector, IF1): the IF is turret-only and MUST stay invisible to the converter.
        let e = as_element(&stats("CV", "8\"w", &["TUR(2/2/1,IF1)"]), "Demolisher", 4);
        assert!(!e.has_sua("IF"));
        assert_eq!(e.sua_num("IF"), 0.0);
        assert_eq!(e.turret_suas.get("IF"), Some(&SuaVal::Num(1.0)));
        assert_eq!(
            e.turret_suas.get("TUR"),
            Some(&SuaVal::Dmg(DamageVector { s: 2.0, m: 2.0, l: Some(1.0), e: None }))
        );
    }

    #[test]
    fn turret_multiple_interior_suas() {
        let e = as_element(&stats("CV", "6\"t", &["TUR(2/2/-,SRM2/2,TAG)"]), "x", 4);
        assert!(e.suas.is_empty());
        assert_eq!(
            e.turret_suas.get("SRM"),
            Some(&SuaVal::Dmg(DamageVector { s: 2.0, m: 2.0, l: None, e: None }))
        );
        assert_eq!(e.turret_suas.get("TAG"), Some(&SuaVal::Flag));
    }

    #[test]
    fn decimal_transport_and_doors() {
        let e = as_element(&stats("CV", "6\"t", &["IT1.95", "CT0.009", "CAR4", "CT5-D2"]), "x", 4);
        assert_eq!(e.sua_num("IT"), 1.95);
        assert_eq!(e.sua_num("CAR"), 4.0);
        // Both a decimal CT and a doored CT resolve to the numeric transport value.
        assert_eq!(e.suas.get("CT"), Some(&SuaVal::Num(5.0))); // "CT5-D2" parsed last, overwrites
    }

    #[test]
    fn if_minimal_value() {
        let e = as_element(&stats("BM", "6\"", &["IF0*"]), "x", 4);
        assert_eq!(e.sua_num("IF"), 0.5);
    }

    #[test]
    fn artillery_split_on_last_dash() {
        let e = as_element(&stats("CV", "6\"t", &["ARTAIS-2", "ARTCM5-1"]), "x", 4);
        assert_eq!(e.suas.get("ARTAIS"), Some(&SuaVal::Art(2)));
        assert_eq!(e.suas.get("ARTCM5"), Some(&SuaVal::Art(1)));
    }

    #[test]
    fn c3_masters_key_on_base_code() {
        // C3M2/C3BSM2 carry a value, but MegaMek's enum KEY is C3M/C3BSM (hasSUA true), so we key
        // on the base code (internal digit preserved) rather than burying the whole token — the
        // converter's AC3 network test reads hasSUA(C3M)/hasSUA(C3BSM).
        let e = as_element(&stats("BM", "6\"", &["C3M2", "C3BSM2", "C3I", "C3S"]), "x", 4);
        assert_eq!(e.suas.get("C3M"), Some(&SuaVal::Num(2.0)));
        assert_eq!(e.suas.get("C3BSM"), Some(&SuaVal::Num(2.0)));
        assert!(e.has_sua("C3M"));
        assert!(!e.has_sua("C3M2")); // NOT keyed on the whole token
        // value-1 forms stay presence flags:
        assert_eq!(e.suas.get("C3I"), Some(&SuaVal::Flag));
        assert_eq!(e.suas.get("C3S"), Some(&SuaVal::Flag));
    }

    #[test]
    fn station_keeping_movement() {
        // Large-craft station-keeping "0.2k" → value 2, mode "k" (not primary 0 with a garbage mode).
        assert_eq!(parse_movement("0.2k"), (2, 0, "k".to_string()));
    }

    #[test]
    fn presence_suas_ending_in_digit() {
        // NC3 (Naval C3), BHJ2/BHJ3 (HarJel II/III) are presence-only; the converter reads them via
        // hasSUA, so they must stay whole and must NOT inject a false split base code.
        let e = as_element(&stats("BM", "6\"", &["NC3", "BHJ2", "BHJ3"]), "x", 4);
        assert_eq!(e.suas.get("NC3"), Some(&SuaVal::Flag));
        assert_eq!(e.suas.get("BHJ2"), Some(&SuaVal::Flag));
        assert_eq!(e.suas.get("BHJ3"), Some(&SuaVal::Flag));
        assert!(!e.has_sua("BHJ")); // must NOT inject a false base BHJ (converter reads IfAll BHJ)
        assert!(!e.has_sua("NC"));
    }

    #[test]
    fn tsemp_o_keeps_value() {
        // TSEMP-O1 (TSEMPO) is value-bearing, unlike the valueless I-TSM.
        let e = as_element(&stats("BM", "6\"", &["TSEMP-O1", "I-TSM"]), "x", 4);
        assert_eq!(e.suas.get("TSEMP-O"), Some(&SuaVal::Num(1.0)));
        assert_eq!(e.suas.get("I-TSM"), Some(&SuaVal::Flag));
    }

    #[test]
    fn flags_and_skipped_locals() {
        let e = as_element(&stats("BM", "6\"", &["ENE", "CASE", "STL", "CF:6", "IMMOBILE", "I-TSM"]), "x", 4);
        assert_eq!(e.suas.get("ENE"), Some(&SuaVal::Flag));
        assert_eq!(e.suas.get("CASE"), Some(&SuaVal::Flag));
        assert_eq!(e.suas.get("STL"), Some(&SuaVal::Flag));
        assert_eq!(e.suas.get("I-TSM"), Some(&SuaVal::Flag)); // hyphen flag kept whole
        assert!(!e.suas.contains_key("CF")); // colon token skipped
        assert!(!e.suas.contains_key("IMMOBILE")); // whitelist-skipped
    }

    #[test]
    fn unit_type_mapping() {
        assert_eq!(sbf_type_from_tp("BM"), SbfElementType::Bm);
        assert_eq!(sbf_type_from_tp("IM"), SbfElementType::Bm);
        assert_eq!(sbf_type_from_tp("AF"), SbfElementType::As);
        assert_eq!(sbf_type_from_tp("SV"), SbfElementType::V); // tp-only: ground SV (aerodyne → As in as_element)
        assert_eq!(sbf_type_from_tp("BD"), SbfElementType::V);
        assert_eq!(sbf_type_from_tp("ZZ"), SbfElementType::La); // default
        assert!(SbfElementType::Bm.is_ground());
        assert!(SbfElementType::As.is_aerospace());
        assert!(SbfElementType::Unknown.is_aerospace()); // broad test
    }

    #[test]
    fn aerodyne_support_vehicle_routes_to_aerospace() {
        // isAerospaceSV(): a bare SV code is ground, but an aerodyne SV is a fixed-wing aircraft → As.
        assert_eq!(as_element(&stats("SV", "5a", &[]), "Fixed-Wing", 4).sbf_type, SbfElementType::As);
        // A ground Support Vehicle (hover/tracked/wheeled/…) stays V.
        assert_eq!(as_element(&stats("SV", "22\"h", &[]), "Truck", 4).sbf_type, SbfElementType::V);
        // Non-SV aerodyne units (fighters) are unaffected by the refinement.
        assert_eq!(as_element(&stats("AF", "10a", &[]), "ASF", 4).sbf_type, SbfElementType::As);
    }

    #[test]
    fn fuel_rating_fighter_zero() {
        let mut s = stats("AF", "8a", &["FUEL0"]);
        assert_eq!(as_element(&s, "x", 4).fuel_rating(), 4.0); // fighter, 0 fuel → 4
        s.specials = vec!["FUEL20".into()];
        assert_eq!(as_element(&s, "x", 4).fuel_rating(), 20.0);
        let g = stats("BM", "6\"", &[]); // ground, no fuel
        assert_eq!(as_element(&g, "x", 4).fuel_rating(), 0.0);
    }

    #[test]
    fn jround_java_parity() {
        assert_eq!(jround(2.5), 3);
        assert_eq!(jround(-2.5), -2); // Java floor(x+0.5), not Rust round (which gives -3)
        assert_eq!(jround(2.4), 2);
        assert_eq!(jround(0.5), 1);
    }
}
