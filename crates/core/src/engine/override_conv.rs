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

//! BattleTech: Override card conversion (DFA Wargaming's fan ruleset).
//!
//! Turns a parsed unit into an Override record card: weapons packed into ≤9 **TICs** (fire
//! groups), with per-TIC damage/heat/range derived from Total Warfare stats. Decoded from DFA's
//! client-side converter and validated against `data/override-goldens/`. See
//! `docs/override-conversion.md` for the full spec; the `_v5` names track Override rules v5.x.
//!
//! This module is the pure algorithm core: it takes resolved weapon inputs (a DB key + location)
//! and produces card rows. The `Mech` → input adapter lives separately so the algorithm can be
//! golden-tested without the full unit model.

use crate::domain::{Location, Mech, MotiveType, UnitType};
use crate::engine::movement::{target_movement_modifier, MoveMode};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One entry in DFA's weapon database. All values are stored as strings (as in the source JSON);
/// use the numeric accessors. Absent fields deserialize to empty strings.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct WeaponStats {
    pub name: String,
    pub fullname: String,
    #[serde(default)]
    pub tech: String,
    /// `E` energy, `B` ballistic, `M` missile, `P` physical.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// Total Warfare damage; Override base = `ceil(damage/3)`.
    #[serde(default)]
    pub damage: String,
    /// Total Warfare heat; TIC heat = `round(Σheat/5)`.
    #[serde(default)]
    pub heat: String,
    #[serde(default)]
    pub crits: String,
    // Pre-baked Override bracket modifiers. `0/2/4…` = `+N`; `>6` (DB uses `9`) = no shot. Pulse
    // weapons carry negatives (e.g. `-2`).
    #[serde(rename = "rangePB", default)]
    pub range_pb: String,
    #[serde(rename = "rangeS", default)]
    pub range_s: String,
    #[serde(rename = "rangeM", default)]
    pub range_m: String,
    #[serde(rename = "rangeL", default)]
    pub range_l: String,
    #[serde(rename = "rangeX", default)]
    pub range_x: String,
    /// Count of M (missile/cluster) dice.
    #[serde(rename = "damageM", default)]
    pub damage_m: String,
    #[serde(rename = "shiftM", default)]
    pub shift_m: String,
    #[serde(rename = "damageAdj", default)]
    pub damage_adj: String,
    /// Variable-by-bracket damage (SNPPC, MML, HAG) → rendered `a|b|c`.
    #[serde(rename = "varPBSdamage", default)]
    pub var_pbs: String,
    #[serde(rename = "varMdamage", default)]
    pub var_m: String,
    #[serde(rename = "varLXdamage", default)]
    pub var_lx: String,
    /// Cluster weapon (LB-X / HAG) → `+C` notation.
    #[serde(rename = "useC", default)]
    pub use_c: String,
    /// Heat-damage weapon (flamer) → `+H`.
    #[serde(rename = "useH", default)]
    pub use_h: String,
    /// Rapid-fire (UAC/RAC) → `(RF)` tag; value is the rack/shots figure.
    #[serde(rename = "useR", default)]
    pub use_r: String,
    /// Compatible fire control, csv: `aiv` (Artemis IV), `av` (Artemis V), `apollo`.
    #[serde(rename = "useFCS", default)]
    pub use_fcs: String,
    #[serde(rename = "useTC", default)]
    pub use_tc: String,
    #[serde(rename = "useAmmo", default)]
    pub use_ammo: String,
    /// Csv tags: `lrm srm mrm atm rl srt lrt var hag20..40 ai zerobase ssrm slrm sbg`.
    #[serde(default)]
    pub specials: String,
    #[serde(rename = "baOnly", default)]
    pub ba_only: bool,
}

impl WeaponStats {
    fn num(s: &str) -> i32 {
        s.parse().unwrap_or(0)
    }
    /// Total Warfare base damage value.
    pub fn tw_damage(&self) -> i32 {
        Self::num(&self.damage)
    }
    /// Total Warfare heat value.
    pub fn tw_heat(&self) -> i32 {
        Self::num(&self.heat)
    }
    /// Number of M (missile/cluster) dice this weapon contributes.
    pub fn m_dice(&self) -> i32 {
        Self::num(&self.damage_m)
    }
    /// Whether a `specials` tag is present (case-insensitive, comma-separated list).
    pub fn has_special(&self, tag: &str) -> bool {
        self.specials
            .split(',')
            .any(|t| t.trim().eq_ignore_ascii_case(tag))
    }
    /// Whether this fire-control tag is supported (`aiv`/`av`/`apollo`).
    pub fn supports_fcs(&self, fcs: &str) -> bool {
        self.use_fcs
            .split(',')
            .any(|t| t.trim().eq_ignore_ascii_case(fcs))
    }
    /// `true` for rapid-fire weapons (Ultra/Rotary ACs).
    pub fn is_rapid(&self) -> bool {
        Self::num(&self.use_r) > 0
    }
    /// `true` for cluster weapons (LB-X, HAG) that render `+C`.
    pub fn is_cluster(&self) -> bool {
        self.use_c == "1"
    }
}

/// The embedded 240-entry weapon DB, keyed by internal id (`mlas`, `ppc`, `lrm20`, `clb10x`, …).
/// Source: `data/override-goldens/reference/override_weapons.json`, extracted verbatim from DFA's
/// client bundle (chunk 833).
pub fn weapon_db() -> &'static HashMap<String, WeaponStats> {
    static DB: OnceLock<HashMap<String, WeaponStats>> = OnceLock::new();
    DB.get_or_init(|| {
        const RAW: &str =
            include_str!("../../../../data/override-goldens/reference/override_weapons.json");
        serde_json::from_str(RAW).expect("override weapon DB is valid JSON")
    })
}

/// Look up a weapon by its DB id.
pub fn weapon(key: &str) -> Option<&'static WeaponStats> {
    weapon_db().get(key)
}

// ── Row rendering (`groupData`) ────────────────────────────────────────────────────────────────

/// A weapon resolved for packing/rendering: a DB entry plus where it's mounted and which
/// unit-level fire-control applies to it. `use_*` flags are the *effective* ones (unit equips the
/// system AND this weapon supports it).
#[derive(Debug, Clone)]
pub struct WeaponInst {
    pub stats: &'static WeaponStats,
    /// Display location code (`"LA"`, `"RA"`, `"T"`, `"HD"`, `"TU"`, `"FR"`, `"N"`, …).
    pub location: String,
    pub rear: bool,
    pub use_tc: bool,
    pub use_aiv: bool,
    pub use_av: bool,
    pub use_apollo: bool,
    pub use_aes: bool,
    pub use_os: bool,
    /// Firing arc (biped/quad: `rear ? Rear : Any`; vehicles/aero set per location).
    pub arc: Arc,
    /// Assigned TIC (1..=9; 0 = unassigned). Set by [`pack`].
    pub tic: u8,
}

impl WeaponInst {
    /// Resolve a weapon by DB id at a location, using the biped/quad arc rule
    /// (`rear ? Rear : Any`). Fire-control flags default off.
    pub fn new(key: &str, location: &str, rear: bool) -> Option<Self> {
        Some(Self {
            stats: weapon(key)?,
            location: location.to_string(),
            rear,
            use_tc: false,
            use_aiv: false,
            use_av: false,
            use_apollo: false,
            use_aes: false,
            use_os: false,
            arc: if rear { Arc::Rear } else { Arc::Any },
            tic: 0,
        })
    }
    fn damage_max(&self) -> i32 {
        self.stats.tw_damage() + WeaponStats::num(&self.stats.damage_adj)
    }
    fn is_var(&self) -> bool {
        self.stats.has_special("var")
    }
}

/// Firing arc of a weapon (Override has no mech "side" arcs — torso twist makes everything `Any`
/// except rear-mounted). Vehicles/aero assign Front/Rear/Left/Right by location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arc {
    Front,
    Rear,
    Left,
    Right,
    Any,
}

/// Unit-level context the packer consults (fire-control the unit actually mounts, BA squad size,
/// Destiny net-heat sinks). All-default = a plain unit with no special systems.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnitCtx {
    pub has_tc: bool,
    pub has_aiv: bool,
    pub has_av: bool,
    pub has_apollo: bool,
    pub has_aes: bool,
    /// Battle-armor squad size multiplier on damage (`None` for non-BA).
    pub squad_size: Option<i32>,
    /// Destiny mode net-heat sinks (0 = standard mode, the only mode current DFA ships).
    pub destiny_sinks: i32,
}

/// A single rendered weapons row of an Override card.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TicRow {
    pub name: String,
    /// Damage string as printed (`"7"`, `"4+M4 (14)"`, `"1+C3"`, `"2+C5|4|3"`).
    pub damage: String,
    /// Heat (`round(Σtw_heat/5)`); `None` for vehicles, which omit the column.
    pub heat: i32,
    pub location: String,
    /// Range bracket strings (`"+0"`, `"+2"`, `"-2"`, `"--"`).
    pub pb: String,
    pub s: String,
    pub m: String,
    pub l: String,
    pub x: String,
}

fn ceil3(n: i32) -> i32 {
    (f64::from(n) / 3.0).ceil() as i32
}
fn round5(n: i32) -> i32 {
    (f64::from(n) / 5.0).round() as i32
}
fn render_bracket(v: i32) -> String {
    if v < 0 {
        v.to_string() // pulse to-hit bonus, e.g. "-2"
    } else if v > 6 {
        "--".to_string() // DB sentinel (9) → no shot in this bracket
    } else {
        format!("+{v}")
    }
}

/// Render one TIC (a group of weapons that fire together) into a card row. Faithful port of DFA's
/// `groupData`; covers plain / cluster (`+C`) / missile (`+M`) / heat-damage (`+H`) / variable
/// (`a|b|c`, incl. HAG) weapons. (Flechette and PPC-capacitor specials are deferred.)
pub fn group_data(weapons: &[WeaponInst]) -> TicRow {
    if weapons.is_empty() {
        return TicRow::default();
    }
    let mut names: Vec<(String, u32)> = Vec::new(); // insertion-ordered counts
    let (mut damage, mut damage_m, mut damage_max, mut damage_c) = (0, 0, 0, 0);
    let (mut use_c, mut use_h, mut use_m, mut heat) = (0, 0, 0, 0);
    let (mut var_pbs, mut var_m, mut var_lx, mut use_var_c, mut hag_base) = (0, 0, 0, 0, 0);
    let (mut r_pb, mut r_s, mut r_m, mut r_l, mut r_x) =
        (i32::MIN, i32::MIN, i32::MIN, i32::MIN, i32::MIN);
    let mut locs: Vec<String> = Vec::new();

    for w in weapons {
        let st = w.stats;
        match names.iter_mut().find(|(n, _)| *n == st.name) {
            Some((_, c)) => *c += 1,
            None => names.push((st.name.clone(), 1)),
        }
        if st.has_special("hag20") {
            hag_base = 2;
        } else if st.has_special("hag30") {
            hag_base = 3;
        } else if st.has_special("hag40") {
            hag_base = 4;
        }
        damage_max += w.damage_max();
        let is_var = st.has_special("var");
        if st.m_dice() > 0 {
            let a = ceil3(w.damage_max()) - st.m_dice();
            use_m += ceil3(a) + WeaponStats::num(&st.shift_m);
            if is_var {
                var_pbs += WeaponStats::num(&st.var_pbs);
                var_m += WeaponStats::num(&st.var_m);
                var_lx += WeaponStats::num(&st.var_lx);
            } else {
                damage_m += st.m_dice() - WeaponStats::num(&st.shift_m);
                if st.has_special("zerobase") {
                    damage_m = 0;
                }
            }
        } else if is_var {
            if st.use_c == "1" {
                use_var_c += 1;
            }
            var_pbs += WeaponStats::num(&st.var_pbs);
            var_m += WeaponStats::num(&st.var_m);
            var_lx += WeaponStats::num(&st.var_lx);
        } else {
            let s = st.tw_damage();
            damage += s;
            if st.use_c == "1" {
                damage_c += s;
                use_c += 1;
            }
        }
        use_h += WeaponStats::num(&st.use_h);
        heat += st.tw_heat();
        if !locs.contains(&w.location) {
            locs.push(w.location.clone());
        }
        r_pb = r_pb.max(WeaponStats::num(&st.range_pb));
        r_s = r_s.max(WeaponStats::num(&st.range_s));
        r_m = r_m.max(WeaponStats::num(&st.range_m));
        r_l = r_l.max(WeaponStats::num(&st.range_l));
        r_x = r_x.max(WeaponStats::num(&st.range_x));
    }

    // Name: "x2 Foo, Bar" + effective fire-control / special tags.
    let mut name = String::new();
    for (n, c) in &names {
        let part = if *c > 1 {
            format!("x{c} {n}")
        } else {
            n.clone()
        };
        if name.is_empty() {
            name = part;
        } else {
            name.push_str(", ");
            name.push_str(&part);
        }
    }
    let all = |f: &dyn Fn(&WeaponInst) -> bool| weapons.iter().all(f);
    if all(&|w| w.use_tc) {
        name.push_str(" (TC)");
    }
    if all(&|w| w.use_aiv) || all(&|w| w.stats.has_special("atm")) {
        name.push_str(" (AIV)");
    }
    if all(&|w| w.use_av) {
        name.push_str(" (AV)");
    }
    if all(&|w| w.use_apollo) {
        name.push_str(" (Apollo)");
    }
    if all(&|w| w.use_os) {
        name.push_str(" (OS)");
    }
    if all(&|w| w.use_aes) {
        name.push_str(" (AES)");
    }
    // (AI) is `some`, not `every` — any anti-infantry weapon flags the whole TIC.
    if weapons.iter().any(|w| w.stats.has_special("ai")) {
        name.push_str(" (AI)");
    }
    if all(&|w| w.stats.is_rapid()) {
        name.push_str(" (RF)");
    }

    // Damage string.
    let damage_str = if use_var_c > 0 {
        let pbs = ceil3(var_pbs) - use_var_c;
        let mm = ceil3(var_m) - use_var_c;
        let lx = ceil3(var_lx) - use_var_c;
        let base = if hag_base > 0 { hag_base } else { use_var_c };
        format!("{base}+C{pbs}|{mm}|{lx}")
    } else {
        damage_c = ceil3(damage_c) - use_c;
        let mut d = if var_pbs != 0 || var_m != 0 || var_lx != 0 {
            let pbs = ceil3(damage + var_pbs) + damage_m - damage_c;
            let mm = ceil3(damage + var_m) + damage_m - damage_c;
            let lx = ceil3(damage + var_lx) + damage_m - damage_c;
            format!("{pbs}|{mm}|{lx}")
        } else {
            (ceil3(damage) + damage_m - damage_c).to_string()
        };
        if use_c > 0 {
            d.push_str(&format!("+C{damage_c}"));
        }
        if use_h > 0 {
            d.push_str(&format!("+H{}", round5(use_h).max(1)));
        }
        if use_m > 0 {
            d.push_str(&format!("+M{use_m} ({})", ceil3(damage_max)));
        }
        d
    };

    // Location: rear weapons prefix the whole row with "(R) ".
    let mut location = locs.join(", ");
    if location.contains("(R)") {
        location = format!("(R) {}", location.replace("(R) ", "").trim());
    }

    // Range: fire-control reduces, AP ammo increases (ammo deferred).
    let (mut pb, mut s, mut m, mut l, mut x) = (r_pb, r_s, r_m, r_l, r_x);
    if all(&|w| w.use_tc) || all(&|w| w.use_apollo) || all(&|w| w.use_aes) {
        pb -= 1;
        s -= 1;
        m -= 1;
        l -= 1;
        x -= 1;
    }

    TicRow {
        name,
        damage: damage_str,
        heat: round5(heat),
        location,
        pb: render_bracket(pb),
        s: render_bracket(s),
        m: render_bracket(m),
        l: render_bracket(l),
        x: render_bracket(x),
    }
}

// ── TIC packing (`autoGroupWeaponsV5`) ───────────────────────────────────────────────────────────

// TIC-level accumulators over a group of weapons (mirror the DFA TIC getters).
fn tic_damage(tic: &[WeaponInst]) -> i32 {
    tic.iter()
        .map(|w| {
            if w.stats.m_dice() == 0 {
                w.stats.tw_damage()
            } else if w.is_var() {
                WeaponStats::num(&w.stats.var_pbs)
            } else {
                0
            }
        })
        .sum()
}
fn tic_damage_m(tic: &[WeaponInst]) -> i32 {
    tic.iter()
        .filter(|w| w.stats.m_dice() > 0 && !w.is_var())
        .map(|w| w.stats.m_dice() - WeaponStats::num(&w.stats.shift_m))
        .sum()
}
fn tic_heat(tic: &[WeaponInst]) -> i32 {
    tic.iter().map(|w| w.stats.tw_heat()).sum()
}
fn tic_has_rapid(tic: &[WeaponInst]) -> bool {
    tic.iter().any(|w| w.stats.is_rapid())
}
fn tic_arcs(tic: &[WeaponInst]) -> Vec<Arc> {
    let mut arcs = Vec::new();
    for w in tic {
        if !arcs.contains(&w.arc) {
            arcs.push(w.arc);
        }
    }
    arcs
}

/// Weapon ordering for packing (`sortV5`): by range profile (X→L→M→S asc, `9` default), then
/// `damageMax` desc, heat asc, location (LA<RA<other), name, rear last. (TIC index is equal at sort
/// time so it's omitted.)
fn sort_v5(a: &WeaponInst, b: &WeaponInst) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    // Empty bracket string sorts as the `9` "no shot" sentinel.
    let by = |w: &WeaponInst, sel: fn(&WeaponStats) -> &String| {
        let s = sel(w.stats);
        if s.is_empty() {
            9
        } else {
            WeaponStats::num(s)
        }
    };
    for sel in [
        (|s: &WeaponStats| &s.range_x) as fn(&WeaponStats) -> &String,
        |s| &s.range_l,
        |s| &s.range_m,
        |s| &s.range_s,
    ] {
        match by(a, sel).cmp(&by(b, sel)) {
            Ordering::Equal => {}
            o => return o,
        }
    }
    match b.damage_max().cmp(&a.damage_max()) {
        Ordering::Equal => {}
        o => return o,
    }
    match a.stats.tw_heat().cmp(&b.stats.tw_heat()) {
        Ordering::Equal => {}
        o => return o,
    }
    let locrank = |w: &WeaponInst| match w.location.as_str() {
        "LA" => 0,
        "RA" => 1,
        _ => 2,
    };
    match locrank(a).cmp(&locrank(b)) {
        Ordering::Equal => {}
        o => return o,
    }
    match a
        .stats
        .name
        .to_lowercase()
        .cmp(&b.stats.name.to_lowercase())
    {
        Ordering::Equal => {}
        o => return o,
    }
    (a.arc == Arc::Rear).cmp(&(b.arc == Arc::Rear)) // rear last
}

fn ceil3f(n: i32) -> f64 {
    (f64::from(n) / 3.0).ceil()
}
fn round5f(n: i32) -> f64 {
    (f64::from(n) / 5.0).round()
}

/// Score how well weapon `w` fits into `tic` at slot index `slot` (`_scoreWeaponForTicV5`).
/// `0.0` = cannot place here; higher = better fit. An empty TIC always accepts (base `100 - slot`).
fn score_weapon_for_tic(ctx: &UnitCtx, w: &WeaponInst, tic: &[WeaponInst], slot: usize) -> f64 {
    let mut n = 100.0 - slot as f64;
    if tic.is_empty() {
        return n;
    }
    let arcs = tic_arcs(tic);
    // Hard gates.
    if w.arc == Arc::Rear && !arcs.contains(&Arc::Rear) {
        return 0.0;
    }
    if w.arc == Arc::Left && !arcs.contains(&Arc::Left) {
        return 0.0;
    }
    if w.arc == Arc::Right && !arcs.contains(&Arc::Right) {
        return 0.0;
    }
    if ctx.has_tc && w.use_tc != tic.iter().all(|t| t.use_tc) {
        return 0.0;
    }
    if ctx.has_aiv && w.use_aiv != tic.iter().all(|t| t.use_aiv) {
        return 0.0;
    }
    if ctx.has_av && w.use_av != tic.iter().all(|t| t.use_av) {
        return 0.0;
    }
    if ctx.has_apollo && w.use_apollo != tic.iter().all(|t| t.use_apollo) {
        return 0.0;
    }
    if ctx.has_aes && w.use_aes != tic.iter().all(|t| t.use_aes) {
        return 0.0;
    }
    for fam in [
        "ssrm", "slrm", "srm", "lrm", "mrm", "atm", "rl", "srt", "lrt", "hag", "sbg",
    ] {
        if w.stats.has_special(fam) != tic.iter().all(|t| t.stats.has_special(fam)) {
            return 0.0;
        }
    }
    if w.stats.is_rapid() != tic_has_rapid(tic) {
        return 0.0;
    }
    if w.use_os != tic.iter().all(|t| t.use_os) {
        return 0.0;
    }
    if w.stats.kind == "P" {
        return 0.0;
    }
    // Base-damage ≤5 cap.
    let mut s = if w.stats.m_dice() > 0 {
        ceil3f(tic_damage(tic)) + f64::from(tic_damage_m(tic)) + f64::from(w.stats.m_dice())
    } else {
        ceil3f(tic_damage(tic) + w.stats.tw_damage()) + f64::from(tic_damage_m(tic))
    };
    if let Some(sq) = ctx.squad_size {
        s *= f64::from(sq);
    }
    if s > 5.0 {
        return 0.0;
    }

    // Soft scoring.
    if arcs.contains(&w.arc) {
        if w.arc == Arc::Any && !tic.is_empty() {
            let same_loc = tic.iter().any(|t| t.location == w.location);
            if !same_loc {
                n *= 0.8;
            }
        }
    } else if arcs.iter().all(|a| *a == Arc::Any) && w.arc == Arc::Front {
        n *= 0.8;
    } else {
        n *= 0.6;
    }
    let rng_eq = |sel: fn(&WeaponStats) -> &String, hit: f64, miss: f64| {
        let a = WeaponStats::num(sel(w.stats));
        let b = WeaponStats::num(sel(tic[0].stats));
        if a == b {
            hit
        } else {
            miss
        }
    };
    n *= rng_eq(|s| &s.range_x, 1.0, 0.1);
    n *= rng_eq(|s| &s.range_l, 1.0, 0.2);
    n *= rng_eq(|s| &s.range_m, 1.0, 0.4);
    n *= rng_eq(|s| &s.range_s, 1.0, 0.8);

    // Heat / damage rounding-efficiency term (this is what rewards heat-saving groups).
    let o = round5f(tic_heat(tic) + w.stats.tw_heat());
    let m = round5f(tic_heat(tic)) + round5f(w.stats.tw_heat());
    let c = m - o;
    let (mut u, mut h) = (0.0, 0.0);
    if w.stats.m_dice() == 0 {
        let t = ceil3f(tic_damage(tic) + w.stats.tw_damage());
        h = ceil3f(tic_damage(tic)) + ceil3f(w.stats.tw_damage());
        u = h - t;
    }
    if c > 0.0 && m > 0.0 {
        let mut e = (m / o.max(1.0)).min(2.0);
        if u > 0.0 && h > 0.0 {
            e = 1.0 + (e - 1.0) * (1.0 - u / h).max(0.0);
        }
        n *= e;
    } else if u > 0.0 && h > 0.0 {
        n *= (1.0 - u / h).max(0.1);
    }
    if ctx.destiny_sinks > 0 {
        let t = round5f(tic_heat(tic) + w.stats.tw_heat()) - f64::from(ctx.destiny_sinks);
        n *= 1.0 - 0.2 * t.clamp(0.0, 5.0);
    }
    n
}

/// Pack weapons into ≤9 TICs (`autoGroupWeaponsV5`): greedy placement in `sort_v5` order, with a
/// bounded displacement search (depth ≤3) that bumps a weapon out of an existing TIC when that
/// yields a better fit. Returns the non-empty TICs, each weapon's `tic` set (1-based).
pub fn pack(ctx: &UnitCtx, mut weapons: Vec<WeaponInst>) -> Vec<Vec<WeaponInst>> {
    weapons.sort_by(sort_v5);
    let mut tics: Vec<Vec<WeaponInst>> = vec![Vec::new(); 9];
    for w in weapons {
        place(ctx, &mut tics, w, 0, usize::MAX);
    }
    tics.retain(|t| !t.is_empty());
    tics
}

fn place(
    ctx: &UnitCtx,
    tics: &mut [Vec<WeaponInst>],
    mut w: WeaponInst,
    depth: u32,
    exclude: usize,
) {
    let scores: Vec<f64> = (0..9)
        .map(|t| score_weapon_for_tic(ctx, &w, &tics[t], t))
        .collect();
    let best = (0..9).fold(0, |b, t| if scores[t] > scores[b] { t } else { b });

    // Displacement: try pulling a weapon out of a populated TIC to fit `w` better there.
    let mut mv: Option<(usize, usize, f64)> = None;
    if depth < 3 {
        #[allow(clippy::needless_range_loop)] // need the index for `exclude` + by-index mutation
        for t in 0..9 {
            if tics[t].len() >= 2 && t != exclude {
                for s in 0..tics[t].len() {
                    let removed = tics[t].remove(s);
                    let m = score_weapon_for_tic(ctx, &w, &tics[t], t);
                    tics[t].insert(s, removed);
                    if m > scores[best] && mv.is_none_or(|(_, _, sc)| m > sc) {
                        mv = Some((t, s, m));
                    }
                }
            }
        }
    }

    if let Some((t, s, _)) = mv {
        let bumped = tics[t].remove(s);
        w.tic = (t + 1) as u8;
        tics[t].push(w);
        place(ctx, tics, bumped, depth + 1, t);
    } else {
        w.tic = (best + 1) as u8;
        tics[best].push(w);
    }
}

// ── Unit-level conversion (§3) ───────────────────────────────────────────────────────────────────
//
// Ported verbatim from DFA's `destinyArmorValue` / `destinyStructureValue` / `damageThreshold`
// getters (chunk 833) plus the card's Move/TMM/Sinks rendering. Every pip count is
// `max(round(a / divisor), 1)`; the card scale is fixed TW/5 (heat *and* sinks).

/// `max(round(a / t), 1)` — the DFA per-location pip reducer.
fn pip(a: i32, t: i32) -> u16 {
    (f64::from(a) / f64::from(t)).round().max(1.0) as u16
}

/// Structure pip count: [`pip`], then halved (min 1) for Composite internal structure.
fn struct_pip(a: i32, t: i32, composite: bool) -> u16 {
    let n = pip(a, t);
    if composite {
        (f64::from(n) / 2.0).round().max(1.0) as u16
    } else {
        n
    }
}

/// Override head-armor pips from TW head armor (`destinyArmorValue` HD branch): `≤2→1, ≤5→2,
/// ≤7→3, else 4`.
fn head_armor_pips(tw: u16) -> u16 {
    match tw {
        0..=2 => 1,
        3..=5 => 2,
        6..=7 => 3,
        _ => 4,
    }
}

/// Whether the unit is treated as airborne for TMM (a +1 bracket): VTOLs and aerospace fighters.
/// (WiGE is left as ground — no golden to pin it.)
fn is_airborne(mech: &Mech) -> bool {
    mech.is_aerospace() || mech.motive == Some(MotiveType::Vtol)
}

/// The Override TMM for a single movement value: the TW target-movement bracket (jump adds its own
/// +1) plus +1 when airborne.
fn tmm_value(mp: u8, jumped: bool, airborne: bool) -> i32 {
    let mode = if jumped {
        MoveMode::Jumped
    } else {
        MoveMode::Stationary
    };
    target_movement_modifier(mp, mode) + i32::from(airborne)
}

/// Single-letter motive code suffixed to a vehicle's run value (`6t`, `14v`). Only `t`/`v` are
/// golden-verified; the rest follow the same first-letter convention.
fn motive_suffix(m: MotiveType) -> &'static str {
    match m {
        MotiveType::Tracked => "t",
        MotiveType::Wheeled => "w",
        MotiveType::Hover => "h",
        MotiveType::Vtol => "v",
        MotiveType::Naval => "n",
        MotiveType::Wige => "g",
    }
}

/// Card "Type" label. Mechs branch on Mekbay subtype (Omni/Industrial); everything else is fixed.
fn type_label(mech: &Mech) -> String {
    match mech.unit_type {
        UnitType::Aerospace => "Aerospace Fighter".into(),
        UnitType::Vehicle => "Combat Vehicle".into(),
        UnitType::Infantry => "Infantry".into(),
        UnitType::BattleArmor => "Battle Armor".into(),
        UnitType::Mech => {
            if mech.subtype.contains("Omni") {
                "OmniMech".into()
            } else if mech.subtype.contains("Industrial") {
                "IndustrialMech".into()
            } else {
                "BattleMech".into()
            }
        }
    }
}

/// Heat-sink dissipation in card units (`round(dissipation / 5)`), matching the 0–5 heat ladder.
fn sinks(dissipation: u16) -> u16 {
    (f64::from(dissipation) / 5.0).round() as u16
}

/// Aerospace damage Threshold: `max(round(((LW+RW)/2 + Nose + Aft) / 30), 1)` over raw TW arc
/// armor (`get damageThreshold`).
pub fn aero_dthr(mech: &Mech) -> i32 {
    let a = |loc: Location| i32::from(mech.armor.get(&loc).map_or(0, |x| x.armor_max));
    let (n, lw, rw, aft) = (
        a(Location::Nose),
        a(Location::LeftWing),
        a(Location::RightWing),
        a(Location::Aft),
    );
    let e = f64::from(lw + rw) / 2.0 + f64::from(n + aft);
    ((e / 30.0).round() as i32).max(1)
}

/// The card's unit-data panel (everything outside the weapons table and armor diagram).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitData {
    pub type_label: String,
    pub mass: u16,
    /// Movement line: mech `"4 / 6"` (+ `" / 4j"` when it jumps), vehicle `"4 / 6t"`, aero `"5"`.
    pub move_line: String,
    /// TMM line, field-parallel to [`Self::move_line`]; aero is a single value.
    pub tmm_line: String,
    /// `round(dissipation / 5)`; `None` for vehicles (which drop heat entirely).
    pub sinks: Option<u16>,
    /// Aerospace damage threshold; `None` otherwise.
    pub d_thr: Option<i32>,
    /// Whether the 0–5 heat ladder is shown (mech/aero; vehicles omit it).
    pub heat_scale: bool,
    /// Aero labels the panel `Thrust`/`DThr` and uses single move/TMM values; ground uses `Move`/`TMM`.
    pub aero: bool,
}

/// Convert a unit into its Override card unit-data panel.
pub fn unit_data(mech: &Mech) -> UnitData {
    if mech.is_aerospace() {
        let thrust = mech.walk;
        return UnitData {
            type_label: type_label(mech),
            mass: mech.tonnage,
            move_line: thrust.to_string(),
            tmm_line: tmm_value(thrust, false, true).to_string(),
            sinks: Some(sinks(mech.dissipation)),
            d_thr: Some(aero_dthr(mech)),
            heat_scale: true,
            aero: true,
        };
    }
    let vehicle = mech.is_vehicle();
    let airborne = is_airborne(mech);
    let suffix = if vehicle {
        mech.motive.map(motive_suffix).unwrap_or("")
    } else {
        ""
    };
    let mut move_line = format!("{} / {}{}", mech.walk, mech.run, suffix);
    let mut tmm_line = format!(
        "{} / {}",
        tmm_value(mech.walk, false, airborne),
        tmm_value(mech.run, false, airborne)
    );
    if mech.jump > 0 {
        move_line.push_str(&format!(" / {}j", mech.jump));
        tmm_line.push_str(&format!(" / {}", tmm_value(mech.jump, true, airborne)));
    }
    UnitData {
        type_label: type_label(mech),
        mass: mech.tonnage,
        move_line,
        tmm_line,
        sinks: if vehicle {
            None
        } else {
            Some(sinks(mech.dissipation))
        },
        d_thr: None,
        heat_scale: !vehicle,
        aero: false,
    }
}

/// One region of the Override armor diagram, after conversion. A mech's three torsos collapse into
/// a single region (front pips summed, structure from `CT + 2·ST`), with [`Self::rear`] holding the
/// merged rear-armor pips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmorRegion {
    /// Representative location (mech torso region → `CenterTorso`; aero SI → `AeroSI`).
    pub loc: Location,
    pub label: String,
    pub armor: u16,
    pub structure: u16,
    /// Rear-armor pips (mech torso only).
    pub rear: Option<u16>,
}

/// Convert a unit's per-location armor/structure into Override diagram regions.
pub fn override_armor(mech: &Mech) -> Vec<ArmorRegion> {
    let at = |loc: Location| mech.armor.get(&loc).copied().unwrap_or_default();

    if mech.is_aerospace() {
        let mut out: Vec<ArmorRegion> = [
            Location::Nose,
            Location::LeftWing,
            Location::RightWing,
            Location::Aft,
        ]
        .into_iter()
        .map(|loc| ArmorRegion {
            loc,
            label: loc.label().into(),
            armor: pip(i32::from(at(loc).armor_max), 4),
            structure: 0,
            rear: None,
        })
        .collect();
        // Shared structural integrity: FU = max(thrust, floor(0.1·tonnage)), then round(FU/3).
        let fu = mech.walk.max((0.1 * f64::from(mech.tonnage)).floor() as u8);
        out.push(ArmorRegion {
            loc: Location::AeroSI,
            label: Location::AeroSI.label().into(),
            armor: 0,
            structure: pip(i32::from(fu), 3),
            rear: None,
        });
        return out;
    }

    if mech.is_vehicle() {
        // Uniform structure = max(round(ceil(tonnage/10) / 3), 1).
        let chunks = (i32::from(mech.tonnage) + 9) / 10; // ceil(tonnage / 10)
        let vs = pip(chunks, 3);
        return mech
            .locations()
            .into_iter()
            .map(|loc| ArmorRegion {
                loc,
                label: loc.label().into(),
                armor: pip(i32::from(at(loc).armor_max), 4),
                structure: vs,
                rear: None,
            })
            .collect();
    }

    if mech.unit_type != UnitType::Mech {
        return Vec::new(); // infantry / battle armor: out of v1 scope
    }

    let composite = mech.structure_type.contains("Composite");
    let mut out = Vec::new();

    let hd = at(Location::Head);
    out.push(ArmorRegion {
        loc: Location::Head,
        label: "Head".into(),
        armor: head_armor_pips(hd.armor_max),
        structure: struct_pip(i32::from(hd.internal_max), 3, composite),
        rear: None,
    });

    let (ct, lt, rt) = (
        at(Location::CenterTorso),
        at(Location::LeftTorso),
        at(Location::RightTorso),
    );
    out.push(ArmorRegion {
        loc: Location::CenterTorso,
        label: "Torso".into(),
        armor: pip(i32::from(ct.armor_max + lt.armor_max + rt.armor_max), 6),
        // Structure divisor 7 == DFA's `CT + 2·ST` over the three baked torso internals.
        structure: struct_pip(
            i32::from(ct.internal_max + lt.internal_max + rt.internal_max),
            7,
            composite,
        ),
        rear: Some(pip(i32::from(ct.rear_max + lt.rear_max + rt.rear_max), 6)),
    });

    for loc in mech.config.locations() {
        if matches!(
            loc,
            Location::Head | Location::CenterTorso | Location::LeftTorso | Location::RightTorso
        ) {
            continue;
        }
        let a = at(*loc);
        out.push(ArmorRegion {
            loc: *loc,
            label: loc.label().into(),
            armor: pip(i32::from(a.armor_max), 3),
            structure: struct_pip(i32::from(a.internal_max), 3, composite),
            rear: None,
        });
    }
    out
}

// ── Mech → packer adapter (§7) ───────────────────────────────────────────────────────────────────
//
// Resolves a baked unit's weapons against the DB and feeds the packer/renderer. The weapon match
// uses DFA's own `WeaponAlias` map (chunk 833): neurohelmet discards the MegaMek internal id but keeps
// the display name + tech base, and `<tech-prefix><normalized-name>` reconstructs the id the alias
// is keyed on (`"ER Medium Laser"` + Clan → `clermediumlaser` → `cermlas`).

/// DFA's MegaMek-id → DB-key alias map (`WeaponAlias`), keyed by normalized id.
fn weapon_alias() -> &'static HashMap<String, String> {
    static A: OnceLock<HashMap<String, String>> = OnceLock::new();
    A.get_or_init(|| {
        const RAW: &str =
            include_str!("../../../../data/override-goldens/reference/weapon_alias.json");
        serde_json::from_str(RAW).expect("override weapon alias map is valid JSON")
    })
}

/// Normalize a weapon name to DFA's id form: ASCII alphanumerics only, lowercased.
fn normalize_name(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Resolve a neurohelmet weapon (display name + Clan flag) to a DB key via the alias map. Tries the
/// unit's own tech prefix first, then the opposite, then the bare id. Returns `None` for equipment
/// that isn't an Override fire weapon (e.g. Anti-Missile System) or that the DB doesn't carry.
pub fn resolve_key(name: &str, clan: bool) -> Option<&'static str> {
    let n = normalize_name(name);
    let alias = weapon_alias();
    let (own, other) = if clan { ("cl", "is") } else { ("is", "cl") };
    [format!("{own}{n}"), n.clone(), format!("{other}{n}")]
        .iter()
        .find_map(|cand| alias.get(cand))
        .map(String::as_str)
        .filter(|key| weapon(key).is_some())
}

/// The Override weapons-table loc code for a neurohelmet location (mech torsos collapse to `T`; quad
/// front legs act as arms).
fn loc_code(loc: Location) -> &'static str {
    use Location::*;
    match loc {
        Head => "HD",
        CenterTorso | LeftTorso | RightTorso => "T",
        LeftArm | FrontLeftLeg => "LA",
        RightArm | FrontRightLeg => "RA",
        LeftLeg | RearLeftLeg => "LL",
        RightLeg | RearRightLeg => "RL",
        CenterLeg => "CL",
        Front => "FR",
        Rear => "RR",
        LeftSide => "LS",
        RightSide => "RS",
        Turret => "TU",
        FrontTurret => "FT",
        Body => "BD",
        Rotor => "RO",
        Nose => "N",
        LeftWing => "LW",
        RightWing => "RW",
        Aft => "A",
        _ => "?",
    }
}

/// Firing arc for a weapon, by unit type. Mechs torso-twist so everything is `Any` unless rear;
/// vehicles/aero pin arcs by mount location (front/rear/side wings).
fn arc_for(loc: Location, rear: bool, unit_type: UnitType) -> Arc {
    match unit_type {
        UnitType::Vehicle => match loc {
            Location::Front => Arc::Front,
            Location::Rear => Arc::Rear,
            Location::LeftSide => Arc::Left,
            Location::RightSide => Arc::Right,
            _ => Arc::Any, // turret / body / rotor fire any direction
        },
        UnitType::Aerospace => match loc {
            Location::Nose => Arc::Front,
            Location::Aft => Arc::Rear,
            Location::LeftWing => Arc::Left,
            Location::RightWing => Arc::Right,
            _ => Arc::Any,
        },
        _ => {
            if rear {
                Arc::Rear
            } else {
                Arc::Any
            }
        }
    }
}

/// Whether the unit mounts a piece of equipment whose name contains `needle`.
fn has_equip(mech: &Mech, needle: &str) -> bool {
    mech.equipment.iter().any(|e| e.name.contains(needle))
}

/// The packer's unit context (fire-control the unit actually mounts). BA squad size and Destiny
/// net-heat are out of v1 scope.
pub fn unit_ctx(mech: &Mech) -> UnitCtx {
    UnitCtx {
        has_tc: mech.has_targeting_computer(),
        has_aiv: has_equip(mech, "Artemis IV"),
        has_av: has_equip(mech, "Artemis V"),
        has_apollo: has_equip(mech, "Apollo"),
        has_aes: false, // Actuator Enhancement gating deferred (no golden)
        squad_size: None,
        destiny_sinks: 0,
    }
}

/// Build the packer inputs from a unit: resolve each mounted weapon to the DB (dropping non-fire
/// equipment), set its arc / loc code / effective fire-control flags, and expand `count` copies.
pub fn weapon_insts(mech: &Mech) -> Vec<WeaponInst> {
    let clan = mech.tech_base.contains("Clan");
    let ctx = unit_ctx(mech);
    let mut out = Vec::new();
    for w in &mech.weapons {
        let Some(key) = resolve_key(&w.name, clan) else {
            continue;
        };
        let Some(stats) = weapon(key) else { continue };
        let code = loc_code(w.location);
        let location = if w.rear {
            format!("(R) {code}")
        } else {
            code.to_string()
        };
        let arc = arc_for(w.location, w.rear, mech.unit_type);
        for _ in 0..w.count.max(1) {
            out.push(WeaponInst {
                stats,
                location: location.clone(),
                rear: w.rear,
                use_tc: ctx.has_tc && w.tc_eligible,
                use_aiv: ctx.has_aiv && stats.supports_fcs("aiv"),
                use_av: ctx.has_av && stats.supports_fcs("av"),
                use_apollo: ctx.has_apollo && stats.supports_fcs("apollo"),
                use_aes: false,
                use_os: false,
                arc,
                tic: 0,
            });
        }
    }
    out
}

/// The ammo lines for the card's Equipment row: `Ammo:<type> (<loc>)`, one per distinct
/// type+location.
fn ammo_lines(mech: &Mech) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for bin in &mech.ammo {
        let ty = bin
            .ammo_key
            .as_deref()
            .and_then(|k| k.split(':').next())
            .unwrap_or(bin.name.as_str());
        let line = format!("Ammo:{ty} ({})", loc_code(bin.location));
        if !out.contains(&line) {
            out.push(line);
        }
    }
    out
}

/// A fully-assembled Override card: the unit-data panel, the packed weapon rows, the armor diagram
/// regions, and the equipment (ammo) lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverrideCard {
    pub unit: UnitData,
    pub tics: Vec<TicRow>,
    pub armor: Vec<ArmorRegion>,
    pub equipment: Vec<String>,
}

/// Convert a baked unit into its complete Override card.
pub fn override_card(mech: &Mech) -> OverrideCard {
    let ctx = unit_ctx(mech);
    let tics = pack(&ctx, weapon_insts(mech))
        .iter()
        .map(|t| group_data(t))
        .collect();
    OverrideCard {
        unit: unit_data(mech),
        tics,
        armor: override_armor(mech),
        equipment: ammo_lines(mech),
    }
}

// ── Override to-hit (QRG attack modifiers) ────────────────────────────────────────────────────────

/// Override attacker-movement to-hit modifier (QRG "Attacker Movement"): standstill (moved < 1")
/// −1, ground 0, jump +2. (Sprint is "no attack"; the shot editor never offers it.)
pub fn ov_attacker_mod(m: MoveMode) -> i32 {
    match m {
        MoveMode::Stationary => -1,
        MoveMode::Jumped => 2,
        MoveMode::Walked | MoveMode::Ran => 0,
    }
}

/// Override target-movement to-hit modifier (QRG "Target Movement"): an immobile / shut-down /
/// unconscious target is a flat −2 (overrides movement); otherwise the target's TMM, +1 if it
/// jumped.
pub fn ov_target_mod(tmm: u8, jumped: bool, immobile: bool) -> i32 {
    if immobile {
        -2
    } else {
        i32::from(tmm) + i32::from(jumped)
    }
}

/// 'Mech physical-attack damage for the card's "Punch / Kick" line (QRG melee table): punch =
/// ⌈Mass/30⌉, kick = ⌈Mass/15⌉. `None` for non-'Mechs (vehicles/aero/infantry don't show it).
pub fn override_physicals(mech: &Mech) -> Option<(u16, u16)> {
    if mech.unit_type != UnitType::Mech {
        return None;
    }
    let t = mech.tonnage;
    Some((t.div_ceil(30), t.div_ceil(15)))
}

/// Parse a rendered range-bracket string back to its numeric to-hit modifier (`"+2"` → 2, `"-2"` →
/// −2). `"--"` (the no-shot sentinel) returns `None`.
pub fn parse_bracket(s: &str) -> Option<i32> {
    if s == "--" {
        None
    } else {
        s.parse().ok()
    }
}

// ── Override critical-hit tables (QRG) ────────────────────────────────────────────────────────────
//
// Transcribed verbatim from the Override Quick Reference. A confirmed structure hit rolls on the
// table for that unit type + location; these drive the crit-popup the player marks results on.

/// The mechanical category of an Override crit result, so marked crits can drive derived effects.
/// Several results are conditional or attacker's-choice in the rulebook (`Ammo`, `Weapon`, `Gyro`,
/// `FuelTank`, …); only [`Self::Actuator`]/[`Self::Motive`] (move/TMM) and [`Self::Engine`] (heat)
/// apply deterministically — the rest surface in the crit-effects summary for the player to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OvCritKind {
    Ammo,
    Weapon,
    Gyro,
    Engine,
    Actuator,
    Motive,
    CrewHit,
    Stunned,
    Avionics,
    FuelTank,
    Bomb,
    Cockpit,
}

/// One row of an Override crit table: the 2d6 (or 1d6, for the 1–6 'Mech/vehicle tables) range as
/// printed, the effect description, and its [`OvCritKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OvCritRow {
    pub roll: &'static str,
    pub effect: &'static str,
    pub kind: OvCritKind,
}

const fn row(roll: &'static str, effect: &'static str, kind: OvCritKind) -> OvCritRow {
    OvCritRow { roll, effect, kind }
}

use OvCritKind::*;

const CRIT_MECH_TORSO: &[OvCritRow] = &[
    row("1", "Ammo (or weapon)", Ammo),
    row("2", "Weapon (attacker's choice)", Weapon),
    row("3-4", "Gyro (+2 PSR, then fall / speed 1)", Gyro),
    row("5-6", "Engine (+1 heat or destroyed)", Engine),
];
const CRIT_MECH_ARM: &[OvCritRow] = &[
    row("1", "Ammo (or weapon)", Ammo),
    row("2-6", "Weapon (attacker's choice)", Weapon),
];
const CRIT_MECH_LEG: &[OvCritRow] = &[
    row("1", "Ammo (or weapon)", Ammo),
    row("2-6", "Actuator (-2 move / -1 TMM)", Actuator),
];
const CRIT_VEH_FRONT: &[OvCritRow] = &[
    row("1", "Crew Hit (pilot damage)", CrewHit),
    row("2", "Stunned (-2 skill rolls next turn)", Stunned),
    row("3-6", "Weapon (attacker's choice)", Weapon),
];
const CRIT_VEH_SIDE: &[OvCritRow] = &[row("1-6", "Motive (-2 move / -1 TMM)", Motive)];
const CRIT_VEH_REAR: &[OvCritRow] = &[
    row("1-2", "Ammo (or motive hit)", Ammo),
    row("3-6", "Motive (-2 move / -1 TMM)", Motive),
];
const CRIT_AERO: &[OvCritRow] = &[
    row("2", "Nose Weapon", Weapon),
    row("3", "Avionics (+2 PSR)", Avionics),
    row("4", "Fuel Tank (destroyed on 10+)", FuelTank),
    row("5-6", "Right Wing Weapon", Weapon),
    row("7", "Engine (+1 heat or destroyed)", Engine),
    row("8-9", "Left Wing Weapon", Weapon),
    row("10", "Ammo", Ammo),
    row("11", "Bomb Disabled (or reroll)", Bomb),
    row("12", "Cockpit (pilot damage)", Cockpit),
];

/// The Override critical-hit table for a region, by unit type and location. `None` for regions that
/// don't roll crits (e.g. the aerospace SI pool, which has no own table).
pub fn crit_table(mech: &Mech, loc: Location) -> Option<&'static [OvCritRow]> {
    use Location::*;
    if mech.is_aerospace() {
        return match loc {
            Nose | LeftWing | RightWing | Aft => Some(CRIT_AERO),
            _ => None,
        };
    }
    if mech.is_vehicle() {
        return match loc {
            Front | Turret | FrontTurret => Some(CRIT_VEH_FRONT),
            LeftSide | RightSide | FrontLeftSide | FrontRightSide | RearLeftSide
            | RearRightSide => Some(CRIT_VEH_SIDE),
            Rear => Some(CRIT_VEH_REAR),
            _ => None,
        };
    }
    match loc {
        // The merged 'Mech torso region is keyed on CenterTorso.
        CenterTorso => Some(CRIT_MECH_TORSO),
        LeftArm | RightArm => Some(CRIT_MECH_ARM),
        LeftLeg | RightLeg | FrontLeftLeg | FrontRightLeg | RearLeftLeg | RearRightLeg
        | CenterLeg => Some(CRIT_MECH_LEG),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{LocationArmor, MechConfig, WeaponMount};
    use std::collections::BTreeMap;

    fn inst(key: &str, loc: &str) -> WeaponInst {
        WeaponInst::new(key, loc, false).unwrap_or_else(|| panic!("weapon {key} present"))
    }
    /// Group several copies of one weapon at a location.
    fn group(key: &str, loc: &str, n: usize) -> Vec<WeaponInst> {
        (0..n).map(|_| inst(key, loc)).collect()
    }

    #[test]
    fn db_loads_all_weapons() {
        let db = weapon_db();
        assert_eq!(db.len(), 240, "expected the full 240-weapon DB");
    }

    #[test]
    fn known_entries_parse() {
        // Medium Laser: TW dmg 5 → base 2; TW heat 3 → row heat 1.
        let mlas = weapon("mlas").expect("mlas present");
        assert_eq!(mlas.fullname, "Medium Laser");
        assert_eq!(mlas.tw_damage(), 5);
        assert_eq!(mlas.tw_heat(), 3);

        // AC/20: TW dmg 20 → base ceil(20/3)=7 (exceeds the ≤5 group cap → own TIC).
        let ac20 = weapon("ac20").expect("ac20 present");
        assert_eq!(ac20.tw_damage(), 20);

        // LB 10-X is a cluster weapon.
        assert!(weapon("lb10x").unwrap().is_cluster());
        // RAC/5 is rapid-fire.
        assert!(weapon("rac5").unwrap().is_rapid());
        // cLRM-20 carries M dice + the lrm special.
        let clrm = weapon("clrm20").unwrap();
        assert_eq!(clrm.m_dice(), 2);
        assert!(clrm.has_special("lrm"));
    }

    // Each case below reproduces a row from a committed golden card (data/override-goldens/).

    #[test]
    fn ac20_single_tic() {
        // Hunchback HBK-4G: AC/20 → "7", heat 1, M+2.
        let row = group_data(&group("ac20", "T", 1));
        assert_eq!(row.damage, "7");
        assert_eq!(row.heat, 1);
        assert_eq!(
            (row.pb.as_str(), row.s.as_str(), row.m.as_str()),
            ("+0", "+0", "+2")
        );
    }

    #[test]
    fn x2_medium_lasers_group() {
        // Hunchback: x2 MLas → "4", heat round(6/5)=1.
        let row = group_data(&group("mlas", "LA", 2));
        assert_eq!(row.name, "x2 MLas");
        assert_eq!(row.damage, "4");
        assert_eq!(row.heat, 1);
    }

    #[test]
    fn x2_clan_lrm20_missile() {
        // Mad Cat Prime: x2 cLRM-20 → "4+M4 (14)", heat 2.
        let row = group_data(&group("clrm20", "T", 2));
        assert_eq!(row.name, "x2 cLRM-20");
        assert_eq!(row.damage, "4+M4 (14)");
        assert_eq!(row.heat, 2);
    }

    #[test]
    fn lb10x_cluster() {
        // Bushwacker: LB 10-X → "1+C3".
        let row = group_data(&group("lb10x", "RA", 1));
        assert_eq!(row.damage, "1+C3");
    }

    #[test]
    fn chag20_variable_cluster() {
        // Vulture (Mad Dog) F: cHAG 20 → "2+C5|4|3".
        let row = group_data(&group("chag20", "LA", 1));
        assert_eq!(row.damage, "2+C5|4|3");
    }

    #[test]
    fn rac5_rapid_fire_tag() {
        // Rifleman RFL-8D: RAC/5 → "3 (RF)".
        let row = group_data(&group("rac5", "LA", 1));
        assert_eq!(row.damage, "3");
        assert!(row.name.ends_with("(RF)"), "got {:?}", row.name);
    }

    #[test]
    fn cmplas_pulse_negative_bracket() {
        // Mad Cat: cMPLas → "3", PB -2, S -2.
        let row = group_data(&group("cmplas", "T", 1));
        assert_eq!(row.damage, "3");
        assert_eq!(row.heat, 1);
        assert_eq!((row.pb.as_str(), row.s.as_str()), ("-2", "-2"));
    }

    #[test]
    fn mg_anti_infantry_tag() {
        // Goliath / Mad Cat: x2 MG → "(AI)" tag, heat 0.
        let row = group_data(&group("mg", "T", 2));
        assert!(row.name.contains("(AI)"), "got {:?}", row.name);
        assert_eq!(row.heat, 0);
    }

    // End-to-end: pack a real loadout, render each TIC, compare to the golden card.
    fn packed_rows(weapons: Vec<WeaponInst>) -> Vec<(String, String, i32)> {
        let mut rows: Vec<_> = pack(&UnitCtx::default(), weapons)
            .iter()
            .map(|t| {
                let r = group_data(t);
                (r.name, r.damage, r.heat)
            })
            .collect();
        rows.sort();
        rows
    }

    #[test]
    fn hunchback_hbk4g_end_to_end() {
        // Card (data/override-goldens/.../btd_hunchback_hbk4g): AC/20 alone, x2 MLas across both
        // arms (grouped via the heat-efficiency bonus), Small Laser alone.
        let weapons = vec![
            inst("mlas", "LA"),
            inst("mlas", "RA"),
            inst("ac20", "T"),
            inst("slas", "HD"),
        ];
        let mut expected = vec![
            ("AC/20".to_string(), "7".to_string(), 1),
            ("x2 MLas".to_string(), "4".to_string(), 1),
            ("SLas".to_string(), "1".to_string(), 0),
        ];
        expected.sort();
        assert_eq!(packed_rows(weapons), expected);
    }

    #[test]
    fn mad_cat_prime_dense_packing() {
        // Card (btd_madcat_prime): ER Larges stay split (4+4>5), ER Mediums split (3+3>5), the two
        // LRM-20s group (4+M4), the two MGs group, pulse stays solo. 7 TICs.
        let weapons = vec![
            inst("clrm20", "T"),
            inst("clrm20", "T"),
            inst("cmg", "T"),
            inst("cmg", "T"),
            inst("cerllas", "LA"),
            inst("cerllas", "RA"),
            inst("cermlas", "LA"),
            inst("cermlas", "RA"),
            inst("cmplas", "T"),
        ];
        let mut expected = vec![
            ("cerLLas".to_string(), "4".to_string(), 2),
            ("cerLLas".to_string(), "4".to_string(), 2),
            ("cerMLas".to_string(), "3".to_string(), 1),
            ("cerMLas".to_string(), "3".to_string(), 1),
            ("cMPLas".to_string(), "3".to_string(), 1),
            ("x2 cLRM-20".to_string(), "4+M4 (14)".to_string(), 2),
            ("x2 cMG (AI)".to_string(), "2".to_string(), 0),
        ];
        expected.sort();
        assert_eq!(packed_rows(weapons), expected);
    }

    // ── Unit-level conversion (§3) ───────────────────────────────────────────────────────────────

    /// `LocationArmor { armor, rear, internal }` shorthand.
    fn la(armor: u16, rear: u16, internal: u16) -> LocationArmor {
        LocationArmor {
            armor_max: armor,
            rear_max: rear,
            internal_max: internal,
        }
    }
    fn armor_map(entries: &[(Location, LocationArmor)]) -> BTreeMap<Location, LocationArmor> {
        entries.iter().copied().collect()
    }

    #[test]
    fn hunchback_unit_data() {
        // Card: BattleMech, 50t, Move 4 / 6, TMM 1 / 2, Sinks 3 (13 single HS). Heat ladder shown.
        let m = Mech {
            tonnage: 50,
            walk: 4,
            run: 6,
            jump: 0,
            dissipation: 13,
            ..Mech::default()
        };
        let d = unit_data(&m);
        assert_eq!(d.type_label, "BattleMech");
        assert_eq!(d.mass, 50);
        assert_eq!(d.move_line, "4 / 6");
        assert_eq!(d.tmm_line, "1 / 2");
        assert_eq!(d.sinks, Some(3));
        assert!(d.heat_scale && !d.aero);
        assert_eq!(d.d_thr, None);
    }

    #[test]
    fn omnimech_type_label() {
        // Mad Cat Prime: subtype carries "Omni" → card "Type: OmniMech".
        let m = Mech {
            subtype: "BattleMek Omni".into(),
            ..Mech::default()
        };
        assert_eq!(unit_data(&m).type_label, "OmniMech");
    }

    #[test]
    fn rifleman_jump_tmm() {
        // Card: Move 4 / 6 / 4j, TMM 1 / 2 / 2 — jumping a 4 brackets to 1 then +1 for the jump.
        let m = Mech {
            tonnage: 60,
            walk: 4,
            run: 6,
            jump: 4,
            dissipation: 20,
            ..Mech::default()
        };
        let d = unit_data(&m);
        assert_eq!(d.move_line, "4 / 6 / 4j");
        assert_eq!(d.tmm_line, "1 / 2 / 2");
        assert_eq!(d.sinks, Some(4));
    }

    #[test]
    fn manticore_vehicle_unit_data() {
        // Card: Combat Vehicle, Move 4 / 6t (tracked suffix), TMM 1 / 2, no Sinks / heat ladder.
        let m = Mech {
            tonnage: 60,
            walk: 4,
            run: 6,
            unit_type: UnitType::Vehicle,
            motive: Some(MotiveType::Tracked),
            dissipation: 0,
            ..Mech::default()
        };
        let d = unit_data(&m);
        assert_eq!(d.type_label, "Combat Vehicle");
        assert_eq!(d.move_line, "4 / 6t");
        assert_eq!(d.tmm_line, "1 / 2");
        assert_eq!(d.sinks, None);
        assert!(!d.heat_scale);
    }

    #[test]
    fn warrior_vtol_is_airborne() {
        // Card: VTOL, Move 9 / 14v, TMM 4 / 5 — the airborne +1 over ground brackets (3 / 4).
        let m = Mech {
            tonnage: 21,
            walk: 9,
            run: 14,
            unit_type: UnitType::Vehicle,
            motive: Some(MotiveType::Vtol),
            ..Mech::default()
        };
        let d = unit_data(&m);
        assert_eq!(d.move_line, "9 / 14v");
        assert_eq!(d.tmm_line, "4 / 5");
        assert_eq!(d.mass, 21); // true tonnage — we do not copy DFA's VTOL mass-display bug
    }

    #[test]
    fn stuka_aero_unit_data() {
        // Card: Aerospace Fighter, Thrust 5, TMM 3 (airborne), Sinks 6 (30 single HS), heat ladder.
        // DThr: the literal `damageThreshold` formula yields 6 for this 15-ton armor (84/54/54/48);
        // the captured card shows 5 — a 1-point discrepancy tracked for follow-up.
        let m = Mech {
            tonnage: 100,
            walk: 5,
            unit_type: UnitType::Aerospace,
            dissipation: 30,
            armor: armor_map(&[
                (Location::Nose, la(84, 0, 0)),
                (Location::LeftWing, la(54, 0, 0)),
                (Location::RightWing, la(54, 0, 0)),
                (Location::Aft, la(48, 0, 0)),
            ]),
            ..Mech::default()
        };
        let d = unit_data(&m);
        assert_eq!(d.type_label, "Aerospace Fighter");
        assert_eq!(d.move_line, "5");
        assert_eq!(d.tmm_line, "3");
        assert_eq!(d.sinks, Some(6));
        assert!(d.aero && d.heat_scale);
        assert_eq!(d.d_thr, Some(6));
    }

    #[test]
    fn hunchback_armor_pips() {
        // TW armor (HBK-4G mtf): HD 9; CT 26/5; LT 20/4; RT 20/4; LA/RA 16; LL/RL 20.
        // Internals (50t): HD 3, CT 16, side torso 12, arm 8, leg 12.
        let m = Mech {
            config: MechConfig::Biped,
            armor: armor_map(&[
                (Location::Head, la(9, 0, 3)),
                (Location::CenterTorso, la(26, 5, 16)),
                (Location::LeftTorso, la(20, 4, 12)),
                (Location::RightTorso, la(20, 4, 12)),
                (Location::LeftArm, la(16, 0, 8)),
                (Location::RightArm, la(16, 0, 8)),
                (Location::LeftLeg, la(20, 0, 12)),
                (Location::RightLeg, la(20, 0, 12)),
            ]),
            ..Mech::default()
        };
        let regions = override_armor(&m);
        let get = |loc: Location| regions.iter().find(|r| r.loc == loc).unwrap();
        // Head: 9 → 4 armor (+ structure round(3/3)=1).
        assert_eq!(
            (get(Location::Head).armor, get(Location::Head).structure),
            (4, 1)
        );
        // Torso: round((26+20+20)/6)=11 armor; round((16+12+12)/7)=6 structure; rear round(13/6)=2.
        let t = get(Location::CenterTorso);
        assert_eq!((t.armor, t.structure, t.rear), (11, 6, Some(2)));
        // Arm: round(16/3)=5 armor, round(8/3)=3 structure.
        assert_eq!(
            (
                get(Location::LeftArm).armor,
                get(Location::LeftArm).structure
            ),
            (5, 3)
        );
        // Leg: round(20/3)=7 armor, round(12/3)=4 structure.
        assert_eq!(
            (
                get(Location::LeftLeg).armor,
                get(Location::LeftLeg).structure
            ),
            (7, 4)
        );
    }

    #[test]
    fn manticore_armor_pips() {
        // Vehicle armor /4; uniform structure = round(ceil(60/10)/3) = round(6/3) = 2.
        let m = Mech {
            tonnage: 60,
            unit_type: UnitType::Vehicle,
            motive: Some(MotiveType::Tracked),
            armor: armor_map(&[
                (Location::Front, la(42, 0, 0)),
                (Location::LeftSide, la(33, 0, 0)),
                (Location::RightSide, la(33, 0, 0)),
                (Location::Rear, la(26, 0, 0)),
                (Location::Turret, la(42, 0, 0)),
            ]),
            ..Mech::default()
        };
        let regions = override_armor(&m);
        let get = |loc: Location| regions.iter().find(|r| r.loc == loc).unwrap();
        assert_eq!(get(Location::Front).armor, 11); // round(42/4)=round(10.5)=11
        assert_eq!(get(Location::Rear).armor, 7); // round(26/4)=round(6.5)=7
        assert!(regions.iter().all(|r| r.structure == 2));
    }

    #[test]
    fn stuka_aero_armor_and_si() {
        // Arc armor /4; SI = round(max(thrust 5, floor(0.1·100)=10)/3) = round(10/3) = 3.
        let m = Mech {
            tonnage: 100,
            walk: 5,
            unit_type: UnitType::Aerospace,
            armor: armor_map(&[
                (Location::Nose, la(84, 0, 0)),
                (Location::LeftWing, la(54, 0, 0)),
                (Location::RightWing, la(54, 0, 0)),
                (Location::Aft, la(48, 0, 0)),
            ]),
            ..Mech::default()
        };
        let regions = override_armor(&m);
        let get = |loc: Location| regions.iter().find(|r| r.loc == loc).unwrap();
        assert_eq!(get(Location::Nose).armor, 21); // round(84/4)
        assert_eq!(get(Location::LeftWing).armor, 14); // round(54/4)=round(13.5)=14
        assert_eq!(get(Location::AeroSI).structure, 3);
    }

    // ── Mech → packer adapter (§7) ───────────────────────────────────────────────────────────────

    #[test]
    fn alias_map_loads() {
        assert!(
            weapon_alias().len() > 500,
            "expected the full WeaponAlias map"
        );
    }

    #[test]
    fn resolve_weapon_keys() {
        // IS display names → IS DB keys.
        assert_eq!(resolve_key("Medium Laser", false), Some("mlas"));
        assert_eq!(resolve_key("AC/20", false), Some("ac20"));
        assert_eq!(resolve_key("LB 10-X AC", false), Some("lb10x"));
        assert_eq!(resolve_key("Rotary AC/5", false), Some("rac5"));
        // Tech prefix disambiguates the shared fullname.
        assert_eq!(resolve_key("ER Medium Laser", false), Some("ermlas"));
        assert_eq!(resolve_key("ER Medium Laser", true), Some("cermlas"));
        assert_eq!(resolve_key("Machine Gun", true), Some("cmg"));
        assert_eq!(resolve_key("LRM 20", true), Some("clrm20"));
        assert_eq!(resolve_key("HAG/20", true), Some("chag20"));
        // Anti-Missile System isn't an Override fire weapon → skipped.
        assert_eq!(resolve_key("Anti-Missile System", false), None);
    }

    #[test]
    fn adapter_sets_arc_and_loc() {
        let mut m = Mech {
            tech_base: "Inner Sphere".into(),
            ..Mech::default()
        };
        m.weapons.push(WeaponMount {
            id: 0,
            name: "Medium Laser".into(),
            location: Location::LeftArm,
            rear: false,
            heat: 3,
            damage: "5".into(),
            range: "3/6/9".into(),
            crit_slots: 1,
            ammo_key: None,
            to_hit: 0,
            tc_eligible: true,
            count: 1,
        });
        let insts = weapon_insts(&m);
        assert_eq!(insts.len(), 1);
        assert_eq!(insts[0].location, "LA");
        assert_eq!(insts[0].arc, Arc::Any);
        assert!(!insts[0].use_tc); // no Targeting Computer equipped
    }

    // End-to-end regression against the 10 golden cards. Drives the *real* baked unit through the
    // full adapter→pack→render pipeline and checks every TIC row (name + damage). Skipped when the
    // baked bundle isn't present (e.g. a fresh checkout before `bake`).
    #[test]
    fn golden_cards_end_to_end() {
        use crate::data::bundle::Bundle;
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/mechs.bin");
        let Ok(bundle) = Bundle::load(std::path::Path::new(path)) else {
            eprintln!("skipping golden_cards_end_to_end: {path} not found");
            return;
        };
        let find = |name: &str| -> Mech {
            let idx = bundle.index();
            let pos = idx
                .iter()
                .position(|s| {
                    let dn = if s.model.is_empty() {
                        s.chassis.clone()
                    } else {
                        format!("{} {}", s.chassis, s.model)
                    };
                    dn == name
                })
                .unwrap_or_else(|| panic!("unit {name:?} not in bundle"));
            bundle.get(pos).unwrap().clone()
        };
        // (unit, [(name, damage); per TIC]) — transcribed from data/override-goldens/outputs/*.png.
        let cases: &[(&str, &[(&str, &str)])] = &[
            (
                "Hunchback HBK-4G",
                &[("AC/20", "7"), ("x2 MLas", "4"), ("SLas", "1")],
            ),
            (
                "Mad Cat (Timber Wolf) Prime",
                &[
                    ("cerLLas", "4"),
                    ("cerLLas", "4"),
                    ("x2 cLRM-20", "4+M4 (14)"),
                    ("cerMLas", "3"),
                    ("cerMLas", "3"),
                    ("cMPLas", "3"),
                    ("x2 cMG (AI)", "2"),
                ],
            ),
            (
                "Goliath GOL-1H",
                &[("x2 LRM-10", "2+M2 (7)"), ("PPC", "4"), ("x2 MG (AI)", "2")],
            ),
            (
                "Rifleman RFL-8D",
                &[
                    ("RAC/5 (RF)", "3"),
                    ("RAC/5 (RF)", "3"),
                    ("erMLas", "2"),
                    ("erMLas", "2"),
                ],
            ),
            (
                "Vulture (Mad Dog) F",
                &[
                    ("cHAG 20", "2+C5|4|3"),
                    ("cHAG 20", "2+C5|4|3"),
                    ("cerMLas", "3"),
                    ("cerMLas", "3"),
                    ("cerMLas", "3"),
                    ("cerMLas", "3"),
                ],
            ),
            (
                "Crusader CRD-7W",
                &[
                    ("MML-9 (AIV)", "2|2|1+M2 (5)"),
                    ("MML-9 (AIV)", "2|2|1+M2 (5)"),
                    ("x2 MML-5 (AIV)", "2|2|0+M2 (6)"),
                    ("erMLas", "2"),
                    ("erMLas", "2"),
                ],
            ),
            (
                "Bushwacker BSW-S2",
                &[
                    ("erLLas", "3"),
                    ("LB 10-X", "1+C3"),
                    ("x2 SRM-4", "2+M2 (6)"),
                ],
            ),
            (
                "Manticore Heavy Tank",
                &[
                    ("LRM-10", "1+M1 (4)"),
                    ("PPC", "4"),
                    ("SRM-6", "1+M2 (4)"),
                    ("MLas", "2"),
                ],
            ),
            (
                "Warrior Attack Helicopter H-7C",
                &[("LRM-10", "1+M1 (4)"), ("SRM-4", "1+M1 (3)")],
            ),
            (
                "Stuka STU-K5",
                &[
                    ("LRM-20", "2+M2 (7)"),
                    ("LLas", "3"),
                    ("LLas", "3"),
                    ("LLas", "3"),
                    ("LLas", "3"),
                    ("SRM-4", "1+M1 (3)"),
                    ("MLas", "2"),
                    ("x2 MLas", "4"),
                ],
            ),
        ];
        for (unit, expected) in cases {
            let card = override_card(&find(unit));
            let mut got: Vec<(String, String)> = card
                .tics
                .iter()
                .map(|t| (t.name.clone(), t.damage.clone()))
                .collect();
            let mut want: Vec<(String, String)> = expected
                .iter()
                .map(|(n, d)| (n.to_string(), d.to_string()))
                .collect();
            got.sort();
            want.sort();
            assert_eq!(got, want, "TIC rows mismatch for {unit}");
        }
    }
}
