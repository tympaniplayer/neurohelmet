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

//! Abstract Combat System (ACS) rules — see `docs/acs-implementation-spec.md`. Holds the **Phase-1
//! converter** (SBF Unit → Combat Team → Combat Unit → Formation) and the **Phase-3 combat / morale
//! / fatigue calculators** (to-hit, fractional damage, morale-check TN + Failure Results Table).
//!
//! ACS is the top rung of the BattleForce ladder (planetary-invasion / multi-regiment scale). Its
//! conversion pipeline (IO:BF p.258) bottoms out at Alpha Strike Elements and passes **through SBF
//! Units**, so this module reuses [`super::sbf`] wholesale and adds three aggregation tiers on top:
//!
//! ```text
//! SBF Unit ──Phase 2──► Combat Team ──Phase 3──► Combat Unit ──Phase 4──► Formation
//! (sbf.rs)   (p.261)     (1–4 units)   (pp.261-2)   (2–4 teams)   (p.262)    (1–8 units)
//! ```
//!
//! **There is no MegaMek ACS engine** — IO:BF is the sole authority (same footing as SBF Phase 4 /
//! all of Standard BF). All page cites are btrules extraction markers (`~/dev/btrules/out/…`);
//! printed folio = marker − 2. All rounding is round-half-up ([`jround`]), per IO:BF "round normal".
//!
//! A **Combat Team is structurally an SBF Formation** (the book runs the same Phase-2 steps for
//! "SBF Formation OR ACS Combat Team", p.261), so [`convert_combat_team`] reuses the tested
//! [`super::sbf::convert_formation`] for the shared stats and layers on only the three ACS-specific
//! deltas: the armor step (2E), the ÷3 damage step (2F), and the ÷3 point value (2J).
//!
//! **Scope (v1): ground only.** Aerospace ACS rests on the entire unimplemented Capital-Scale
//! Strategic Aerospace chapter and is a deliberate non-goal (spec Open Q 1). The converter is
//! type-agnostic (it will aggregate aero stat lines correctly), but the session/UI layer flags
//! aerospace Formations as unsupported.

use super::as_element::{jround, DamageVector, SbfElementType, SuaVal};
use super::sbf::{
    capital_range, convert_formation, majority_type, mean, sbf_range_mod, suaval_num, SbfCapital,
    SbfMoveMode, SbfRange, SbfUnit,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A converted ACS **Combat Team** (conversion Phase 2, p.261) — 1–4 SBF Units. A build-time
/// intermediate: where the ÷3 battalion-fusion of armor/damage/PV happens. Derived, immutable; live
/// combat state lives on the session, never here.
#[derive(Clone, Debug, PartialEq)]
pub struct AcsCombatTeam {
    pub name: String,
    pub acs_type: SbfElementType,
    pub size: i64,
    pub movement: i64,
    pub move_mode: SbfMoveMode,
    pub jump_move: i64,
    pub trsp_movement: i64,
    pub trsp_mode: SbfMoveMode,
    pub tmm: i64,
    /// Step 2E: total Unit armor (+SCL#/+PNT#) ÷3, round normal.
    pub armor: i64,
    /// Step 2F: total each band ÷3, round normal; IF÷3 folds into L & E when ≥1.
    pub damage: DamageVector,
    pub tactics: i64,
    pub morale_rating: i64,
    pub skill: i64,
    /// Step 2J: total Unit PV ÷3, round normal.
    pub point_value: i64,
    pub suas: BTreeMap<String, SuaVal>,
    pub units: Vec<SbfUnit>,
}

/// A converted ACS **Combat Unit** (conversion Phase 3, pp.261–262) — 2–4 Combat Teams. **The atom
/// of ACS combat and the tracked object**: it carries the Armor pool, the S/M/L damage line, the
/// three Damage Thresholds, and (at play time) accrues Fatigue. Clan Combat Units are Trinaries —
/// a Clan SBF Formation already *is* the Combat Unit, so Phase 3 is skipped (see
/// [`combat_unit_from_clan_team`]).
#[derive(Clone, Debug, PartialEq)]
pub struct AcsCombatUnit {
    pub name: String,
    pub acs_type: SbfElementType,
    pub size: i64,
    pub movement: i64,
    pub move_mode: SbfMoveMode,
    pub trsp_movement: i64,
    pub trsp_mode: SbfMoveMode,
    /// Step 3D: avg Team TMM + (avg Team JUMP ÷3) — JUMP folds INTO the TMM at this tier.
    pub tmm: i64,
    /// Step 3E: **total** (not averaged) of the Combat Teams' armor.
    pub armor: i64,
    /// Step 3F: **total** each band across Teams (no further ÷3).
    pub damage: DamageVector,
    /// Large-Aerospace multi-arc capital card, if this is a large-craft (`La`) aero Combat Unit —
    /// the aggregated `damage` band is ~0 for large craft (their weapons live on the card), so aero
    /// combat resolves per-arc off this instead. `None` for fighters/ground. A Combat Unit is
    /// abstract (a battalion): it carries the card of its representative large craft.
    pub arcs: Option<crate::domain::ArcCard>,
    pub tactics: i64,
    pub morale_rating: i64,
    /// Step 3H: the 75% / 50% / 25% armor marks; crossing one in a turn triggers a Morale Check.
    pub damage_thresholds: [i64; 3],
    pub skill: i64,
    /// Step 3J: total Team PV ÷3, round normal.
    pub point_value: i64,
    pub suas: BTreeMap<String, SuaVal>,
    pub teams: Vec<AcsCombatTeam>,
}

/// A converted ACS **Formation** (conversion Phase 4, p.262) — 1–8 Combat Units. The grouping /
/// activation / morale-rollup tier; its record sheet (p.237) holds Move, Tactics, Morale, Skill.
/// Ground or Aerospace only, never mixed (p.262 3B). Carries no armor/damage/TMM of its own —
/// combat happens at the Combat Unit.
#[derive(Clone, Debug, PartialEq)]
pub struct AcsFormation {
    pub name: String,
    pub acs_type: SbfElementType,
    /// Step 4C: the **transport** MP of the slowest Combat Unit.
    pub movement: i64,
    pub tactics: i64,
    /// Step 4E: the morale of the **lowest-morale** Combat Unit.
    pub morale_rating: i64,
    pub skill: i64,
    pub point_value: i64,
    pub units: Vec<AcsCombatUnit>,
}

impl AcsFormation {
    /// v1 supports Ground Formations only; aerospace is a deliberate non-goal (spec Open Q 1).
    pub fn is_aerospace(&self) -> bool {
        matches!(self.acs_type, SbfElementType::As | SbfElementType::La)
    }
}

/// Most-restrictive SBF movement mode over an iterator (lowest [`SbfMoveMode::rank`] wins; ties keep
/// the first seen). Mirrors `sbf::most_restrictive`, reimplemented locally over the public `rank`.
fn most_restrictive(modes: impl Iterator<Item = SbfMoveMode>) -> SbfMoveMode {
    modes.fold(SbfMoveMode::Unknown, |cur, m| {
        if m.rank() < cur.rank() {
            m
        } else {
            cur
        }
    })
}

/// Read a `#`-valued SUA from a map as an `f64` (0 when absent).
fn sua_val(suas: &BTreeMap<String, SuaVal>, code: &str) -> f64 {
    suas.get(code).map(suaval_num).unwrap_or(0.0)
}

/// Aggregate SUA maps up a tier (conversion Phase 5, §5B/§5C, pp.262–264): union the flags, **sum**
/// the `#`-valued abilities. This is a display-tier promotion — v1 renders promoted SUAs as
/// reference text and only the calculator-relevant handful are read live (Phase 3). The faithful
/// per-ability ACS-column filtering of the p.262–264 table is deferred until a calculator needs a
/// specific ability; nothing here is load-bearing for the numeric stat lines.
fn aggregate_suas<'a>(
    sources: impl Iterator<Item = &'a BTreeMap<String, SuaVal>>,
) -> BTreeMap<String, SuaVal> {
    let mut acc: BTreeMap<String, (f64, bool)> = BTreeMap::new();
    for m in sources {
        for (k, v) in m {
            let entry = acc.entry(k.clone()).or_insert((0.0, false));
            if !matches!(v, SuaVal::Flag) {
                entry.0 += suaval_num(v);
                entry.1 = true;
            }
        }
    }
    acc.into_iter()
        .map(|(k, (sum, numeric))| {
            if numeric {
                (k, SuaVal::Num(sum as f32))
            } else {
                (k, SuaVal::Flag)
            }
        })
        .collect()
}

/// Combat Team damage (conversion Step 2F, p.261): each band = total across Units ÷3, round normal.
/// `IF` (÷3, round normal) folds into the Long and Extreme bands when ≥1 (p.261). The Extreme band
/// exists only when some Unit carried one (aerospace); ground Units have `l`-and-below only.
fn team_damage(units: &[SbfUnit]) -> DamageVector {
    let sum = |f: fn(&DamageVector) -> f64| units.iter().map(|u| f(&u.damage)).sum::<f64>();
    let s = jround(sum(|d| d.s as f64) / 3.0);
    let m = jround(sum(|d| d.m as f64) / 3.0);
    let mut l = jround(sum(|d| d.l.unwrap_or(0.0) as f64) / 3.0);
    let has_e = units.iter().any(|u| u.damage.e.is_some());
    let mut e = if has_e {
        Some(jround(sum(|d| d.e.unwrap_or(0.0) as f64) / 3.0))
    } else {
        None
    };
    // IF: total the Units' IF specials, ÷3 round normal; if ≥1 add to L (and E when present).
    let indirect = jround(units.iter().map(|u| sua_val(&u.suas, "IF")).sum::<f64>() / 3.0);
    if indirect >= 1 {
        l += indirect;
        e = e.map(|x| x + indirect);
    }
    DamageVector {
        s: s as f32,
        m: m as f32,
        l: Some(l as f32),
        e: e.map(|x| x as f32),
    }
}

/// Convert 1–4 SBF Units into an ACS **Combat Team** (conversion Phase 2, p.261).
///
/// A Combat Team runs the same aggregation as an SBF Formation for type/size/movement/TMM/tactics/
/// morale/skill/SUAs, so we lean on the tested [`convert_formation`] and override only the three
/// ACS deltas: armor (Step 2E), damage (Step 2F), and point value (Step 2J, ÷3 vs the SBF sum).
pub fn convert_combat_team(name: &str, units: &[SbfUnit]) -> AcsCombatTeam {
    let f = convert_formation(name, units);

    // Step 2E — armor: total Unit armor (+SCL#/+PNT#) ÷3, round normal.
    let scl_pnt: f64 = units
        .iter()
        .map(|u| sua_val(&u.suas, "SCL") + sua_val(&u.suas, "PNT"))
        .sum();
    let armor_total: f64 = units.iter().map(|u| u.armor as f64).sum::<f64>() + scl_pnt;
    let armor = jround(armor_total / 3.0);

    // Step 2F — damage; Step 2J — point value.
    let damage = team_damage(units);
    let point_value = jround(units.iter().map(|u| u.point_value as f64).sum::<f64>() / 3.0);

    // FLK grant (p.261): total Unit Flak; ≥2 → set the FLK flag for display.
    let mut suas = f.suas;
    let flk: f64 = units
        .iter()
        .filter_map(|u| u.suas.get("FLK"))
        .map(|v| match v {
            SuaVal::Dmg(d) => (d.m + d.l.unwrap_or(0.0)) as f64,
            other => suaval_num(other),
        })
        .sum();
    if flk >= 2.0 {
        suas.entry("FLK".to_string()).or_insert(SuaVal::Flag);
    }

    AcsCombatTeam {
        name: name.to_string(),
        acs_type: f.sbf_type,
        size: f.size,
        movement: f.movement,
        move_mode: f.move_mode,
        jump_move: f.jump_move,
        trsp_movement: f.trsp_movement,
        trsp_mode: f.trsp_mode,
        tmm: f.tmm,
        armor,
        damage,
        tactics: f.tactics,
        morale_rating: f.morale_rating,
        skill: f.skill,
        point_value,
        suas,
        units: units.to_vec(),
    }
}

/// The Phase-3 type merges (p.262 Step 3B): Aerospace + Large Aerospace both count as `As`, and
/// Battle-Armor + Conventional Infantry both count as `Ci`, before the predominant-type (≥⅔ else
/// Mixed Ground) rule is applied.
fn merge_type_phase3(t: SbfElementType) -> SbfElementType {
    match t {
        SbfElementType::La => SbfElementType::As,
        SbfElementType::Ba => SbfElementType::Ci,
        other => other,
    }
}

/// The shared Phase-3 body: build a Combat Unit from already-built Combat Teams. Both the standard
/// path ([`convert_combat_unit`]) and the Clan Trinary path ([`combat_unit_from_clan_team`]) land
/// here; they differ only in how the Teams are assembled upstream.
fn combat_unit_from_teams(name: &str, teams: &[AcsCombatTeam]) -> AcsCombatUnit {
    // Step 3B — type (with the As/La and Ba/Ci merges).
    let types: Vec<SbfElementType> = teams
        .iter()
        .map(|t| merge_type_phase3(t.acs_type))
        .collect();
    let acs_type = majority_type(&types);
    let is_aero = matches!(acs_type, SbfElementType::As | SbfElementType::La);

    // Step 3C — size.
    let size = jround(mean(teams.iter().map(|t| t.size as f64)));

    // Step 3D — movement (aero → lowest thrust; ground → mean); TMM folds JUMP in.
    let movement = if is_aero {
        teams.iter().map(|t| t.movement).min().unwrap_or(0)
    } else {
        jround(mean(teams.iter().map(|t| t.movement as f64)))
    };
    let move_mode = most_restrictive(teams.iter().map(|t| t.move_mode));
    let trsp_movement = jround(mean(teams.iter().map(|t| t.trsp_movement as f64)));
    let trsp_mode = most_restrictive(teams.iter().map(|t| t.trsp_mode));
    let tmm = jround(mean(teams.iter().map(|t| t.tmm as f64)))
        + jround(mean(teams.iter().map(|t| t.jump_move as f64)) / 3.0);

    // Step 3E — armor: TOTAL of team armors (not averaged).
    let armor: i64 = teams.iter().map(|t| t.armor).sum();

    // Step 3F — damage: TOTAL each band across teams.
    let band_sum = |f: fn(&DamageVector) -> f64| teams.iter().map(|t| f(&t.damage)).sum::<f64>();
    let has_e = teams.iter().any(|t| t.damage.e.is_some());
    let damage = DamageVector {
        s: band_sum(|d| d.s as f64) as f32,
        m: band_sum(|d| d.m as f64) as f32,
        l: Some(band_sum(|d| d.l.unwrap_or(0.0) as f64) as f32),
        e: has_e.then(|| band_sum(|d| d.e.unwrap_or(0.0) as f64) as f32),
    };

    // Step 3I — skill (needed by tactics/morale).
    let skill = jround(mean(teams.iter().map(|t| t.skill as f64)));

    // SUAs (Step 5C aggregation) — needed for the MHQ tactics term.
    let suas = aggregate_suas(teams.iter().map(|t| &t.suas));

    // Step 3G — tactics: base 10 − Move, ±skill vs 4, MHQ ÷2 (cap 2) subtracted, floor 0.
    let mut tactics = (10 - movement + skill - 4).max(0);
    let mhq = sua_val(&suas, "MHQ");
    if mhq > 0.0 {
        tactics -= 2.min(jround(mhq / 2.0));
    }
    let tactics = tactics.max(0);

    // Step 3H — morale + the three damage thresholds (75/50/25% marks).
    let morale_rating = 3 + skill;
    let step = (armor as f64 * 0.25).floor() as i64;
    let damage_thresholds = [armor - step, armor - 2 * step, armor - 3 * step];

    // Step 3J — point value: total team PV ÷3, round normal.
    let point_value = jround(teams.iter().map(|t| t.point_value as f64).sum::<f64>() / 3.0);

    // The capital card of the Combat Unit's representative large craft (a `La` SBF Unit). Gated on
    // `La`, not merely `arcs.is_some()` — Small Craft carry a card but are standard `As` (see the
    // SBF Phase-3 gate); ACS aero fire on that card resolves per-arc.
    let arcs = teams
        .iter()
        .flat_map(|t| &t.units)
        .find(|u| u.sbf_type == SbfElementType::La)
        .and_then(|u| u.arcs.clone());

    AcsCombatUnit {
        name: name.to_string(),
        acs_type,
        size,
        movement,
        move_mode,
        trsp_movement,
        trsp_mode,
        tmm,
        armor,
        damage,
        arcs,
        tactics,
        morale_rating,
        damage_thresholds,
        skill,
        point_value,
        suas,
        teams: teams.to_vec(),
    }
}

/// Convert 2–4 Combat Teams into an ACS **Combat Unit** (conversion Phase 3, pp.261–262).
pub fn convert_combat_unit(name: &str, teams: &[AcsCombatTeam]) -> AcsCombatUnit {
    combat_unit_from_teams(name, teams)
}

/// The Clan path (p.262 Step 3A): a Clan Combat Unit **is** a Trinary — the Clan SBF Formation from
/// the prior phases already is the Combat Unit, so Phase 3 is skipped. The single Combat Team formed
/// from that Clan Formation's Units becomes the sole team of a one-team Combat Unit.
pub fn combat_unit_from_clan_team(name: &str, team: AcsCombatTeam) -> AcsCombatUnit {
    combat_unit_from_teams(name, &[team])
}

/// Convert 1–8 Combat Units into an ACS **Formation** (conversion Phase 4, p.262).
pub fn convert_formation_acs(name: &str, units: &[AcsCombatUnit]) -> AcsFormation {
    // Step 4B — type (Ground/Aerospace; the As/La & Ba/Ci merges carry up).
    let types: Vec<SbfElementType> = units
        .iter()
        .map(|u| merge_type_phase3(u.acs_type))
        .collect();
    let acs_type = majority_type(&types);

    // Step 4C — movement: transport MP of the SLOWEST Combat Unit.
    let movement = units.iter().map(|u| u.trsp_movement).min().unwrap_or(0);

    // Step 4F — skill (needed by tactics).
    let skill = jround(mean(units.iter().map(|u| u.skill as f64)));

    // Step 4D — tactics: base 10 − Move, ±skill, subtract highest Unit MHQ (cap 2), floor 0.
    let mut tactics = (10 - movement + skill - 4).max(0);
    let max_mhq = units
        .iter()
        .map(|u| sua_val(&u.suas, "MHQ"))
        .fold(0.0f64, f64::max);
    if max_mhq > 0.0 {
        tactics -= 2.min(max_mhq as i64);
    }
    let tactics = tactics.max(0);

    // Step 4E — morale: the LOWEST Combat Unit morale.
    let morale_rating = units.iter().map(|u| u.morale_rating).min().unwrap_or(0);

    let point_value = units.iter().map(|u| u.point_value).sum();

    AcsFormation {
        name: name.to_string(),
        acs_type,
        movement,
        tactics,
        morale_rating,
        skill,
        point_value,
        units: units.to_vec(),
    }
}

// ============================ Phase 3: combat / morale / fatigue ============================
// Where neurohelmet owns the rules (IO:BF pp.248–250). Pure functions + ctx structs (the SBF
// `sbf_to_hit` pattern). Single-force, boardless: positional Master-Modifier rows are hand-set via
// each ctx's `misc_mod` escape hatch, exactly like SBF's terrain toggle. neurohelmet never rolls — the
// morale functions are *readouts* over the player's own 2D6.

/// ACS morale rungs (IO:BF p.250) — **six** states, wider than SBF's four (`Unsteady`, `Retreating`
/// and `Surrender` are RAW here, not the ACAR artifact SBF dropped). Manual, player-set; the
/// morale-check calculator is a readout that reads the Failure Results Table. Ordinals 0→6, worst
/// last; `Surrender` = combat-ineffective (counts as destroyed for VP). Persisted on the session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AcsMorale {
    #[default]
    Normal,
    Shaken,
    Unsteady,
    Broken,
    Retreating,
    Routed,
    Surrender,
}

impl AcsMorale {
    /// The Master-Modifier "Atk To-Hit" column (p.241): when a rattled unit is the **attacker**, its
    /// own to-hit worsens by this much. Broken/Retreating cap the meaningful ladder; Surrender can't
    /// attack (max penalty).
    pub fn own_attack_mod(self) -> i64 {
        match self {
            AcsMorale::Normal => 0,
            AcsMorale::Shaken => 1,
            AcsMorale::Unsteady => 2,
            AcsMorale::Broken => 3,
            AcsMorale::Retreating | AcsMorale::Routed | AcsMorale::Surrender => 4,
        }
    }

    /// The Master-Modifier "Target" column (p.241): a rattled **target** is easier to hit, so the
    /// attacker's TN drops by this much (the negative of [`Self::own_attack_mod`]).
    pub fn as_target_mod(self) -> i64 {
        -self.own_attack_mod()
    }

    /// The Master-Modifier "Damage" column for a rattled attacker (p.241): Broken −0.2,
    /// Retreating/Routed −0.4 to damage dealt.
    pub fn damage_dealt_mod(self) -> f32 {
        match self {
            AcsMorale::Broken => -0.2,
            AcsMorale::Retreating | AcsMorale::Routed | AcsMorale::Surrender => -0.4,
            _ => 0.0,
        }
    }

    /// Short display label for the rung.
    pub fn label(self) -> &'static str {
        match self {
            AcsMorale::Normal => "Normal",
            AcsMorale::Shaken => "Shaken",
            AcsMorale::Unsteady => "Unsteady",
            AcsMorale::Broken => "Broken",
            AcsMorale::Retreating => "Retreating",
            AcsMorale::Routed => "Routed",
            AcsMorale::Surrender => "Surrender",
        }
    }

    /// The six rungs worst-last, for cycling in the UI (Surrender wraps back to Normal).
    pub const ALL: [AcsMorale; 7] = [
        AcsMorale::Normal,
        AcsMorale::Shaken,
        AcsMorale::Unsteady,
        AcsMorale::Broken,
        AcsMorale::Retreating,
        AcsMorale::Routed,
        AcsMorale::Surrender,
    ];

    /// Advance one rung worse, wrapping Surrender → Normal (the manual morale cycle key).
    pub fn cycled(self) -> AcsMorale {
        let i = AcsMorale::ALL.iter().position(|&m| m == self).unwrap_or(0);
        AcsMorale::ALL[(i + 1) % AcsMorale::ALL.len()]
    }
}

/// A Combat Unit's experience rating (IO:BF p.260). Distinct from the numeric Skill value; derived
/// from it via [`Self::from_skill`] unless the player overrides. Drives the to-hit, morale and
/// fatigue tables.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AcsExperience {
    WetBehindTheEars,
    ReallyGreen,
    Green,
    #[default]
    Regular,
    Veteran,
    Elite,
    Heroic,
    Legendary,
}

impl AcsExperience {
    /// Map a numeric Skill value to the experience rating (Experience Skill Value Table, p.260:
    /// 7 = Wet Behind the Ears … 0 = Legendary). Out-of-range clamps.
    pub fn from_skill(skill: i64) -> Self {
        match skill.clamp(0, 7) {
            7 => AcsExperience::WetBehindTheEars,
            6 => AcsExperience::ReallyGreen,
            5 => AcsExperience::Green,
            4 => AcsExperience::Regular,
            3 => AcsExperience::Veteran,
            2 => AcsExperience::Elite,
            1 => AcsExperience::Heroic,
            _ => AcsExperience::Legendary,
        }
    }

    /// The Master-Modifier "Attacker To-Hit" column (p.241): +2 (WBTE) … −4 (Legendary).
    pub fn to_hit_mod(self) -> i64 {
        match self {
            AcsExperience::WetBehindTheEars => 2,
            AcsExperience::ReallyGreen => 1,
            AcsExperience::Green => 0,
            AcsExperience::Regular => -1,
            AcsExperience::Veteran => -2,
            AcsExperience::Elite => -3,
            AcsExperience::Heroic => -4,
            AcsExperience::Legendary => -4,
        }
    }

    /// The Master-Modifier "Morale" column (p.241): +2 (WBTE, harder to hold) … −5 (Legendary).
    pub fn morale_mod(self) -> i64 {
        match self {
            AcsExperience::WetBehindTheEars => 2,
            AcsExperience::ReallyGreen => 1,
            AcsExperience::Green => 0,
            AcsExperience::Regular => -1,
            AcsExperience::Veteran => -2,
            AcsExperience::Elite => -3,
            AcsExperience::Heroic => -4,
            AcsExperience::Legendary => -5,
        }
    }

    /// Fatigue Points earned per turn in combat (Fatigue Points Earned Table, p.249).
    pub fn fatigue_earned(self) -> f32 {
        match self {
            AcsExperience::WetBehindTheEars => 2.0,
            AcsExperience::ReallyGreen => 1.0,
            _ => 0.5,
        }
    }

    /// Fatigue Points ignored before any effect (p.249): Elite 2 / Heroic 3 / Legendary 4 (cap 4).
    /// The faction-based ignores (Clan 2, Word of Blake 1) aren't modeled — faction isn't baked.
    pub fn fatigue_ignore(self) -> f32 {
        match self {
            AcsExperience::Elite => 2.0,
            AcsExperience::Heroic => 3.0,
            AcsExperience::Legendary => 4.0,
            _ => 0.0,
        }
    }
}

/// Ground firing-range bracket (p.248) — ACS ground has no Extreme and no Indirect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AcsRange {
    Short,
    #[default]
    Medium,
    Long,
}

impl AcsRange {
    /// Range Modifier Table (p.248): Short −1, Medium +2, Long +4.
    pub fn to_hit_mod(self) -> i64 {
        match self {
            AcsRange::Short => -1,
            AcsRange::Medium => 2,
            AcsRange::Long => 4,
        }
    }

    /// The Combat Unit's damage at this bracket.
    pub fn band(self, d: &DamageVector) -> f32 {
        match self {
            AcsRange::Short => d.s,
            AcsRange::Medium => d.m,
            AcsRange::Long => d.l.unwrap_or(0.0),
        }
    }
}

/// Combat Tactics chosen per Formation (p.248). The level (1–5) is the to-hit sacrifice; each point
/// trades for ±0.1 damage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AcsTactics {
    #[default]
    Standard,
    /// +level to-hit → +0.1·level damage **dealt** on a hit (max +0.5).
    Aggressive(u8),
    /// +level to-hit → −0.1·level damage **received** on a hit (max −0.5).
    Defensive(u8),
}

impl AcsTactics {
    /// The to-hit sacrifice (both Aggressive and Defensive raise the attacker's TN by the level,
    /// capped at 5); Standard is free.
    pub fn to_hit_mod(self) -> i64 {
        match self {
            AcsTactics::Standard => 0,
            AcsTactics::Aggressive(n) | AcsTactics::Defensive(n) => (n as i64).min(5),
        }
    }
}

/// Fatigue band (Fatigue Effects Table, p.249). Computed from a Combat Unit's effective FP.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AcsFatigueBand {
    #[default]
    Rested,
    Tired,
    Flagging,
    Exhausted,
    Spent,
}

/// The fatigue band for a Combat Unit, given its raw accumulated FP and experience (the experience
/// ignore is subtracted first, p.249). Bands: 0–4.5 Rested / 5–8.5 Tired / 9–12.5 Flagging /
/// 13–16.5 Exhausted / 17+ Spent.
pub fn acs_fatigue_band(raw_fp: f32, exp: AcsExperience) -> AcsFatigueBand {
    let fp = (raw_fp - exp.fatigue_ignore()).max(0.0);
    if fp >= 17.0 {
        AcsFatigueBand::Spent
    } else if fp >= 13.0 {
        AcsFatigueBand::Exhausted
    } else if fp >= 9.0 {
        AcsFatigueBand::Flagging
    } else if fp >= 5.0 {
        AcsFatigueBand::Tired
    } else {
        AcsFatigueBand::Rested
    }
}

impl AcsFatigueBand {
    /// The "Combat Mod" column (p.249): +0/+1/+2/+3/+5. Applies to to-hit and morale checks.
    pub fn combat_mod(self) -> i64 {
        match self {
            AcsFatigueBand::Rested => 0,
            AcsFatigueBand::Tired => 1,
            AcsFatigueBand::Flagging => 2,
            AcsFatigueBand::Exhausted => 3,
            AcsFatigueBand::Spent => 5,
        }
    }

    /// The "Damage Mod" column (p.249): 0 / 0 / −0.1 / −0.2 / −0.4.
    pub fn damage_mod(self) -> f32 {
        match self {
            AcsFatigueBand::Rested | AcsFatigueBand::Tired => 0.0,
            AcsFatigueBand::Flagging => -0.1,
            AcsFatigueBand::Exhausted => -0.2,
            AcsFatigueBand::Spent => -0.4,
        }
    }
}

/// Inputs to the ground to-hit calculator (p.248–249). Positional Master-Modifier rows the app
/// doesn't model go through `misc_mod` (hand-set), like SBF's terrain toggle.
#[derive(Clone, Copy, Debug)]
pub struct AcsToHitCtx {
    pub range: AcsRange,
    pub attacker: AcsExperience,
    /// The target's Target Movement Modifier (hand-entered).
    pub target_tmm: i64,
    pub tactics: AcsTactics,
    /// The target's morale rung (a broken target is easier to hit).
    pub target_morale: AcsMorale,
    /// The attacker's own morale rung (a rattled attacker shoots worse).
    pub own_morale: AcsMorale,
    /// The attacker's fatigue band.
    pub fatigue: AcsFatigueBand,
    /// The attacker outmaneuvered the target and attacks from behind (−1).
    pub from_behind: bool,
    /// This is a secondary target for the Formation (+2).
    pub secondary_target: bool,
    /// The Formation is out of supply (+3).
    pub no_supply: bool,
    /// Artillery attack: +2 flat, used **instead of** the range modifier (p.248).
    pub artillery: bool,
    /// Hand-set sum of any other Master-Modifier rows (urban +1, ambush −1, from a Castle Brian −2…).
    pub misc_mod: i64,
}

impl Default for AcsToHitCtx {
    fn default() -> Self {
        AcsToHitCtx {
            range: AcsRange::Medium,
            attacker: AcsExperience::Regular,
            target_tmm: 0,
            tactics: AcsTactics::Standard,
            target_morale: AcsMorale::Normal,
            own_morale: AcsMorale::Normal,
            fatigue: AcsFatigueBand::Rested,
            from_behind: false,
            secondary_target: false,
            no_supply: false,
            artillery: false,
            misc_mod: 0,
        }
    }
}

/// The modified 2D6 target number for a ground attack (p.248–249). `base 4`, then every applicable
/// Master-Modifier row. A natural 2 always misses (the caller enforces that on the roll).
pub fn acs_to_hit(c: &AcsToHitCtx) -> i64 {
    let mut tn = 4;
    tn += if c.artillery { 2 } else { c.range.to_hit_mod() };
    tn += c.attacker.to_hit_mod();
    tn += c.target_tmm;
    tn += c.tactics.to_hit_mod();
    tn += c.target_morale.as_target_mod();
    tn += c.own_morale.own_attack_mod();
    tn += c.fatigue.combat_mod();
    if c.from_behind {
        tn -= 1;
    }
    if c.secondary_target {
        tn += 2;
    }
    if c.no_supply {
        tn += 3;
    }
    tn += c.misc_mod;
    tn
}

/// Inputs to the fractional-damage calculator (p.249).
#[derive(Clone, Copy, Debug)]
pub struct AcsDamageCtx {
    /// The attacker's Combat Tactics (Aggressive adds damage on a hit; other kinds add nothing here).
    pub attacker_tactics: AcsTactics,
    /// Whether the to-hit succeeded — the Aggressive bonus only lands on a hit.
    pub hit: bool,
    /// The target's Defensive tactics level (−0.1 damage received per point).
    pub target_defensive: u8,
    /// Attacked from behind (+0.2, p.248 Step 1).
    pub from_behind: bool,
    /// Secondary target for the Formation (−0.25).
    pub secondary_target: bool,
    /// The attacker's fatigue band (−0.1/−0.2/−0.4).
    pub attacker_fatigue: AcsFatigueBand,
    /// The attacker's morale rung (Broken −0.2, Retreating/Routed −0.4 dealt).
    pub attacker_morale: AcsMorale,
    /// Hand-set sum of any other Damage-column modifiers (ambush +0.2, no-supply −0.1…).
    pub misc_mod: f32,
}

impl Default for AcsDamageCtx {
    fn default() -> Self {
        AcsDamageCtx {
            attacker_tactics: AcsTactics::Standard,
            hit: true,
            target_defensive: 0,
            from_behind: false,
            secondary_target: false,
            attacker_fatigue: AcsFatigueBand::Rested,
            attacker_morale: AcsMorale::Normal,
            misc_mod: 0.0,
        }
    }
}

/// Damage inflicted on the target Combat Unit (p.249): `band_value × (1.0 ± Σmods)`, round normal,
/// floored at 0. The Damage Inflicted Modifier is `1.0` plus the signed sum of every applicable row.
pub fn acs_damage(band_value: f32, c: &AcsDamageCtx) -> i64 {
    let mut m: f64 = 0.0;
    if let AcsTactics::Aggressive(n) = c.attacker_tactics {
        if c.hit {
            m += 0.1 * (n.min(5)) as f64;
        }
    }
    m -= 0.1 * c.target_defensive.min(5) as f64;
    if c.from_behind {
        m += 0.2;
    }
    if c.secondary_target {
        m -= 0.25;
    }
    m += c.attacker_fatigue.damage_mod() as f64;
    m += c.attacker_morale.damage_dealt_mod() as f64;
    m += c.misc_mod as f64;
    jround((band_value as f64 * (1.0 + m)).max(0.0))
}

/// A Combat Unit's current Damage-Threshold band, the column key of the Morale Failure Results
/// Table (p.250). Named by how badly hurt the unit is: `NoDamage` (pristine) → `Pct25` (down to
/// the 25% mark).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcsDamageBand {
    NoDamage,
    NoThreshold,
    Pct75,
    Pct50,
    Pct25,
}

/// Classify a Combat Unit into its Damage-Threshold band from live armor vs its thresholds (p.250).
pub fn acs_damage_band(
    armor_remaining: i64,
    full_armor: i64,
    thresholds: [i64; 3],
) -> AcsDamageBand {
    if armor_remaining >= full_armor {
        AcsDamageBand::NoDamage
    } else if armor_remaining <= thresholds[2] {
        AcsDamageBand::Pct25
    } else if armor_remaining <= thresholds[1] {
        AcsDamageBand::Pct50
    } else if armor_remaining <= thresholds[0] {
        AcsDamageBand::Pct75
    } else {
        AcsDamageBand::NoThreshold
    }
}

/// Inputs to the morale-check calculator (p.249–250).
#[derive(Clone, Copy, Debug)]
pub struct AcsMoraleCtx {
    /// The checking Combat Unit's (or LEAD/COM's) Morale Value.
    pub morale_value: i64,
    pub experience: AcsExperience,
    pub fatigue: AcsFatigueBand,
    /// The unit crossed its third (75%-damage) threshold this turn (+2).
    pub third_threshold: bool,
    /// ≥⅔ of the Formation is at 50% damage or worse (+1).
    pub formation_half_damaged: bool,
    /// The force has suffered an orbital attack (+2; COM rolls for the force).
    pub orbital_attack: bool,
    /// ≥⅔ of the Formation is Shaken or worse (−2).
    pub formation_two_thirds_shaken: bool,
    /// Hand-set extra (e.g. a Combat-Drop penalty = the negative Drop Value).
    pub misc_mod: i64,
}

impl Default for AcsMoraleCtx {
    fn default() -> Self {
        AcsMoraleCtx {
            morale_value: 6,
            experience: AcsExperience::Regular,
            fatigue: AcsFatigueBand::Rested,
            third_threshold: false,
            formation_half_damaged: false,
            orbital_attack: false,
            formation_two_thirds_shaken: false,
            misc_mod: 0,
        }
    }
}

/// The morale-check target number (p.250): the check **holds** on `2D6 ≥ TN`. neurohelmet shows the TN;
/// the player rolls and, on a failure, reads [`acs_morale_result`] for the resulting rung.
pub fn acs_morale_tn(c: &AcsMoraleCtx) -> i64 {
    let mut tn = c.morale_value;
    tn += c.experience.morale_mod();
    tn += c.fatigue.combat_mod();
    if c.third_threshold {
        tn += 2;
    }
    if c.formation_half_damaged {
        tn += 1;
    }
    if c.orbital_attack {
        tn += 2;
    }
    if c.formation_two_thirds_shaken {
        tn -= 2;
    }
    tn += c.misc_mod;
    tn
}

/// Read the Morale Failure Results Table (p.250): the resulting rung from the margin of failure and
/// the unit's current Damage-Threshold band. `margin_of_failure` = TN − roll (≥1 means a failure);
/// a non-positive margin means the check held (returns `Normal`).
pub fn acs_morale_result(band: AcsDamageBand, margin_of_failure: i64) -> AcsMorale {
    if margin_of_failure <= 0 {
        return AcsMorale::Normal;
    }
    use AcsDamageBand::*;
    use AcsMorale::*;
    // Row by margin of failure: 1–3 / 4–6 / 7–9 / 10+.
    let row = if margin_of_failure <= 3 {
        0
    } else if margin_of_failure <= 6 {
        1
    } else if margin_of_failure <= 9 {
        2
    } else {
        3
    };
    // Columns worst→pristine: Pct25, Pct50, Pct75, NoThreshold, NoDamage (Table p.250).
    let table = match band {
        Pct25 => [Broken, Retreating, Routed, Surrender],
        Pct50 => [Unsteady, Retreating, Routed, Surrender],
        Pct75 => [Unsteady, Broken, Retreating, Routed],
        NoThreshold => [Shaken, Unsteady, Broken, Retreating],
        NoDamage => [Shaken, Shaken, Unsteady, Broken],
    };
    table[row]
}

// ============================ Aerospace combat (IO:BF pp.240-241 Master Modifier + p.250 Aerospace
// To-Hit + pp.251-252 Ground Support) ============================
//
// ACS aerospace "uses the Capital-Scale Strategic Aerospace rules except as noted" (p.248), so it
// REUSES the SBF Phase-3 pieces wholesale: `SbfRange` + `sbf_range_mod` (the S+0/M+1/L+2/E+3 aero
// ladder, distinct from the ground `AcsRange` S−1/M+2/L+4), `capital_range` (the −1 bracket for
// capital classes), and `SbfCapital` (the p.191 = p.250 capital weapon-class / high-speed /
// point-defense / screen leg). On top it adds the six large-craft cross-type rows (p.241) and a few
// ACS-only rows, plus the Ground-Support mission calculators. Damage still flows through the existing
// single-pool / Damage-Threshold model — ACS has no crit table (see `acs_damage_band`).

/// The six large-craft cross-type to-hit rows (IO:BF Master Modifier Table, folio p.241), keyed on
/// (attacker craft class → target craft class). Every other pairing (WS-vs-WS, aero-vs-aero, any
/// Space-Station pairing) has NO printed modifier, i.e. `None` = +0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AcsAeroMatchup {
    #[default]
    None,
    AeroVsWarship,
    AeroVsDropship,
    DropshipVsAero,
    DropshipVsWarship,
    WarshipVsAero,
    WarshipVsDropship,
}

impl AcsAeroMatchup {
    pub const ALL: [AcsAeroMatchup; 7] = [
        Self::None,
        Self::AeroVsWarship,
        Self::AeroVsDropship,
        Self::DropshipVsAero,
        Self::DropshipVsWarship,
        Self::WarshipVsAero,
        Self::WarshipVsDropship,
    ];

    /// The to-hit modifier (IO:BF p.241): aero→WS −3, aero→DS −2, DS→aero +2, DS→WS −2, WS→aero +5,
    /// WS→DS −1. No other pairing is printed (+0).
    pub fn to_hit_mod(self) -> i64 {
        match self {
            Self::None => 0,
            Self::AeroVsWarship => -3,
            Self::AeroVsDropship => -2,
            Self::DropshipVsAero => 2,
            Self::DropshipVsWarship => -2,
            Self::WarshipVsAero => 5,
            Self::WarshipVsDropship => -1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AeroVsWarship => "aero → WarShip",
            Self::AeroVsDropship => "aero → DropShip",
            Self::DropshipVsAero => "DropShip → aero",
            Self::DropshipVsWarship => "DropShip → WarShip",
            Self::WarshipVsAero => "WarShip → aero",
            Self::WarshipVsDropship => "WarShip → DropShip",
        }
    }
}

/// Inputs to the ACS aerospace to-hit calculator (folio p.250 Aerospace To-Hit Modifiers Table +
/// p.241 Master Modifier cross-type rows). The capital weapon-class / high-speed / point-defense /
/// screen rows come from the shared [`SbfCapital`] leg (identical to the SBF p.191 subsystem).
#[derive(Clone, Copy, Debug)]
pub struct AcsAeroToHitCtx {
    pub range: SbfRange,
    pub attacker: AcsExperience,
    pub target_tmm: i64,
    /// The six large-craft cross-type rows (p.241).
    pub matchup: AcsAeroMatchup,
    /// The capital-scale leg (weapon class, high-speed, point-defense, screen, atmospheric, …); the
    /// range-bracket reduction for capital classes is applied to `range` before the ladder lookup.
    pub capital: Option<SbfCapital>,
    /// The attacker's own morale rung and fatigue band (ACS attacker state, like the ground calc).
    pub own_morale: AcsMorale,
    pub fatigue: AcsFatigueBand,
    /// Secondary target: +1 (p.250 — distinct from the ground +2).
    pub secondary_target: bool,
    /// Attacker is a Robotic Unit: +1 (p.250).
    pub robotic: bool,
    /// The attacking Formation is itself being attacked by another aerospace Formation: +2 (p.250).
    pub attacked_by_aero: bool,
    /// The target is a Recon Formation: +3 (p.250).
    pub target_recon: bool,
    /// The attacking aerospace Formation is more than 50% Large Aerospace: −2 (p.250).
    pub over_half_large_aero: bool,
    /// Hand-set sum of any remaining Master-Modifier rows the app doesn't model (orbit-to-surface
    /// zone splits, SDS, atmospheric-interface gating, cumulative targeting-damage per hit, …).
    pub misc_mod: i64,
}

impl Default for AcsAeroToHitCtx {
    fn default() -> Self {
        AcsAeroToHitCtx {
            range: SbfRange::Medium,
            attacker: AcsExperience::Regular,
            target_tmm: 0,
            matchup: AcsAeroMatchup::None,
            capital: None,
            own_morale: AcsMorale::Normal,
            fatigue: AcsFatigueBand::Rested,
            secondary_target: false,
            robotic: false,
            attacked_by_aero: false,
            target_recon: false,
            over_half_large_aero: false,
            misc_mod: 0,
        }
    }
}

/// The modified 2D6 target number for an ACS aerospace attack (folio p.250 + p.241): base 4, the ACS
/// attacker framework terms (experience / own morale / fatigue), the aero range ladder, and the
/// aerospace-specific rows. Capital/sub-capital weapons drop the range bracket by 1 first.
pub fn acs_aero_to_hit(c: &AcsAeroToHitCtx) -> i64 {
    let range = match &c.capital {
        Some(cap) => capital_range(c.range, cap.weapon_class),
        None => c.range,
    };
    let mut tn = 4;
    tn += c.attacker.to_hit_mod();
    tn += sbf_range_mod(range);
    tn += c.target_tmm;
    tn += c.matchup.to_hit_mod();
    if let Some(cap) = &c.capital {
        tn += cap.to_hit_mod();
    }
    tn += c.own_morale.own_attack_mod();
    tn += c.fatigue.combat_mod();
    tn += i64::from(c.secondary_target);
    tn += i64::from(c.robotic);
    tn += 2 * i64::from(c.attacked_by_aero);
    tn += 3 * i64::from(c.target_recon);
    tn -= 2 * i64::from(c.over_half_large_aero);
    tn += c.misc_mod;
    tn
}

/// The aerospace damage-lookup range bracket for a shot — the same capital −1 reduction the to-hit
/// applies (IO:BF p.190 "Capital Weapon Ranges").
pub fn acs_aero_range(range: SbfRange, capital: Option<&SbfCapital>) -> SbfRange {
    match capital {
        Some(cap) => capital_range(range, cap.weapon_class),
        None => range,
    }
}

// ---- Aerospace Ground-Support Missions (IO:BF folio pp.251-252) ----

/// CAP / Close Air Support (p.251): a CAP Formation gets −1 to its Engagement Control roll.
pub const ACS_CAP_ENGAGEMENT_MOD: i64 = -1;
/// Aerial Recon (p.251): the base −4 to the Reconnaissance roll (−3 if a hostile aero engages it,
/// +2 if it loses air-to-air — those nuances are table-side).
pub const ACS_AERIAL_RECON_MOD: i64 = -4;
/// Ground Strike / Bombing base to-hit (p.251): the Formation's base TN is its Skill + 3.
pub const ACS_GROUND_STRIKE_TOHIT: i64 = 3;

/// Ground Strike damage per Combat Unit (p.251): one-half the Combat Unit's short-range damage
/// (round normal).
pub fn acs_ground_strike_damage(short: f32) -> i64 {
    jround(short as f64 / 2.0)
}

/// Bomb delivery (p.251): the BOMB rating applied in 5-point clusters — the number of clusters.
pub fn acs_bomb_clusters(bomb: i64) -> i64 {
    if bomb <= 0 {
        0
    } else {
        (bomb + 4) / 5
    }
}

/// Orbit-to-Surface / Surface-to-Orbit PRIMARY damage (p.251): one-quarter (round UP) of the Combat
/// Unit's damage value + 1, minimum 1.
pub fn acs_orbit_to_surface_primary(damage: f32) -> i64 {
    ((damage as f64 / 4.0).ceil() as i64 + 1).max(1)
}

/// Orbit-to-Surface SECONDARY damage (p.251): one-half the primary (round up). Scatter (5-6, same
/// hex) does the same as a successful secondary.
pub fn acs_orbit_to_surface_secondary(primary: i64) -> i64 {
    (primary as f64 / 2.0).ceil() as i64
}

/// One row of the Combat Drop Results Table (IO:BF folio p.251), read by the Margin of Success.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AcsCombatDropResult {
    /// The Drop Value row key (drives follow-on effects and, on a failed drop, the drop-damage %).
    pub drop_value: i64,
    /// Drop Damage: the % of the dropped Combat Units' initial Armor lost on a FAILED drop (0 = none).
    pub drop_damage_pct: u8,
    /// Combat Roll Modifier to the dropped units' first-turn attacks.
    pub combat_roll_mod: i64,
    /// Damage Modifier to the dropped units the turn they land (halve, round down, next turn; drop
    /// once ≤ 0).
    pub damage_mod: f32,
    pub result: &'static str,
}

/// The Combat Drop Results Table (IO:BF folio p.251), keyed on the Combat Drop roll's Margin of
/// Success (roll − TN 6). Better MoS = a tighter drop pattern. The `>12` boundary (folio) is treated
/// as `>12`; a MoS of exactly 12 folds into the next band (a book gap).
pub fn acs_combat_drop_result(mos: i64) -> AcsCombatDropResult {
    let mk =
        |drop_value, drop_damage_pct, combat_roll_mod, damage_mod, result| AcsCombatDropResult {
            drop_value,
            drop_damage_pct,
            combat_roll_mod,
            damage_mod,
            result,
        };
    if mos > 12 {
        mk(5, 0, -4, 0.0, "Parade-ground precision")
    } else if mos >= 9 {
        mk(4, 0, -3, 0.0, "Concentrated avalanche")
    } else if mos >= 6 {
        mk(3, 0, -2, 0.0, "Strong pattern, little scattering")
    } else if mos >= 3 {
        mk(2, 0, -1, 0.0, "Adequate drop pattern")
    } else if mos >= 0 {
        mk(1, 0, 0, -0.1, "Scattered but effective")
    } else if mos >= -3 {
        mk(-1, 0, 1, -0.1, "Poor pattern, moderate scattering")
    } else if mos >= -6 {
        mk(-2, 5, 2, -0.2, "Scattered concentrations")
    } else if mos >= -9 {
        mk(-3, 10, 3, -0.4, "Scattered and disorganized")
    } else if mos >= -12 {
        mk(-4, 15, 4, -0.6, "Scattered beyond recovery")
    } else {
        mk(-5, 20, 5, -0.8, "Unmitigated disaster")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal hand-built SBF Unit fixture — Phase 1 aggregation is tested in isolation from the
    /// (already SBF-tested) element→unit conversion.
    fn unit(name: &str, armor: i64, s: f32, m: f32, l: f32, skill: i64, pv: i64) -> SbfUnit {
        SbfUnit {
            name: name.to_string(),
            sbf_type: SbfElementType::Bm,
            size: 3,
            movement: 4,
            move_mode: SbfMoveMode::MekWalk,
            jump_move: 0,
            trsp_movement: 4,
            trsp_mode: SbfMoveMode::MekWalk,
            tmm: 1,
            armor,
            damage: DamageVector {
                s,
                m,
                l: Some(l),
                e: None,
            },
            arcs: None,
            skill,
            point_value: pv,
            suas: BTreeMap::new(),
        }
    }

    #[test]
    fn team_armor_divides_by_three_round_normal() {
        // Step 2E: 10+10+10 = 30, ÷3 = 10.
        let t = convert_combat_team("t", &vec![unit("a", 10, 0.0, 0.0, 0.0, 4, 30); 3]);
        assert_eq!(t.armor, 10);
        // Round-normal, not truncation: 5+5 = 10, ÷3 = 3.33 → 3.
        let t2 = convert_combat_team("t", &vec![unit("a", 5, 0.0, 0.0, 0.0, 4, 30); 2]);
        assert_eq!(t2.armor, 3);
        // 5×4 = 20, ÷3 = 6.67 → 7 (half-up, would be 6 under truncation).
        let t3 = convert_combat_team("t", &vec![unit("a", 5, 0.0, 0.0, 0.0, 4, 30); 4]);
        assert_eq!(t3.armor, 7);
    }

    #[test]
    fn team_damage_divides_once_by_three() {
        // Ambiguity #6: the ÷3 is applied exactly once. 3+3+3 = 9, ÷3 = 3 (NOT 1).
        let t = convert_combat_team("t", &vec![unit("a", 10, 3.0, 3.0, 3.0, 4, 30); 3]);
        assert_eq!(t.damage.s, 3.0);
        assert_eq!(t.damage.m, 3.0);
        assert_eq!(t.damage.l, Some(3.0));
        // 9+9 = 18, ÷3 = 6.
        let t2 = convert_combat_team("t", &vec![unit("a", 10, 9.0, 0.0, 0.0, 4, 30); 2]);
        assert_eq!(t2.damage.s, 6.0);
    }

    #[test]
    fn team_pv_divides_by_three() {
        // Step 2J: 30+30 = 60, ÷3 = 20 (unlike the SBF Formation which would sum to 60).
        let t = convert_combat_team("t", &vec![unit("a", 10, 0.0, 0.0, 0.0, 4, 30); 2]);
        assert_eq!(t.point_value, 20);
    }

    #[test]
    fn team_morale_is_skill_plus_three() {
        let t = convert_combat_team("t", &vec![unit("a", 10, 0.0, 0.0, 0.0, 3, 30); 2]);
        assert_eq!(t.skill, 3);
        assert_eq!(t.morale_rating, 6);
    }

    #[test]
    fn combat_unit_armor_totals_teams_and_damage_totals() {
        // Step 3E: armor is the TOTAL of team armors (not averaged). Two teams of armor 10 → 20.
        let team = convert_combat_team("t", &vec![unit("a", 15, 4.0, 4.0, 4.0, 4, 30); 2]);
        assert_eq!(team.armor, 10); // 30 ÷ 3
        assert_eq!(team.damage.s, 3.0); // 8 ÷ 3 = 2.67 → 3
        let cu = convert_combat_unit("cu", &[team.clone(), team.clone()]);
        assert_eq!(cu.armor, 20); // Step 3E: 10 + 10
        assert_eq!(cu.damage.s, 6.0); // Step 3F: TOTAL, 3 + 3 (no further ÷3)
    }

    #[test]
    fn combat_unit_damage_thresholds() {
        // Step 3H worked example (p.262): Armor 20 → step 5 → thresholds 15 / 10 / 5.
        let team = convert_combat_team("t", &vec![unit("a", 30, 0.0, 0.0, 0.0, 4, 30); 2]);
        assert_eq!(team.armor, 20); // 60 ÷ 3
        let cu = convert_combat_unit("cu", &[team]);
        assert_eq!(cu.armor, 20);
        assert_eq!(cu.damage_thresholds, [15, 10, 5]);
    }

    #[test]
    fn combat_unit_jump_folds_into_tmm() {
        // Step 3D: TMM = avg(team TMM) + round(avg(team JUMP) ÷ 3).
        let mut u = unit("a", 30, 0.0, 0.0, 0.0, 4, 30);
        u.jump_move = 3;
        u.tmm = 2;
        let team = convert_combat_team("t", &vec![u; 2]);
        assert_eq!(team.tmm, 2);
        assert_eq!(team.jump_move, 3);
        let cu = convert_combat_unit("cu", &[team.clone(), team]);
        // avg TMM 2 + round(3 ÷ 3) = 2 + 1 = 3.
        assert_eq!(cu.tmm, 3);
    }

    #[test]
    fn clan_trinary_skips_phase3() {
        // A Clan Combat Unit is the single Combat Team wrapped as a one-team Combat Unit.
        let team = convert_combat_team("trinary", &vec![unit("a", 30, 6.0, 6.0, 6.0, 3, 45); 3]);
        let cu = combat_unit_from_clan_team("clan-cu", team.clone());
        assert_eq!(cu.teams.len(), 1);
        assert_eq!(cu.armor, team.armor); // single team → total == that team
        assert_eq!(cu.damage.s, team.damage.s);
        assert_eq!(cu.skill, team.skill);
    }

    #[test]
    fn formation_move_is_slowest_transport_and_morale_is_lowest() {
        let team_a = convert_combat_team("a", &vec![unit("a", 30, 4.0, 4.0, 4.0, 4, 30); 2]);
        let mut slow = unit("b", 30, 4.0, 4.0, 4.0, 5, 30); // skill 5 → morale 8, trsp 2
        slow.trsp_movement = 2;
        let team_b = convert_combat_team("b", &vec![slow; 2]);
        let cu_a = convert_combat_unit("cu-a", &[team_a]); // trsp 4, morale 3+4=7
        let cu_b = convert_combat_unit("cu-b", &[team_b]); // trsp 2, morale 3+5=8
        let f = convert_formation_acs("form", &[cu_a.clone(), cu_b.clone()]);
        // Step 4C: slowest transport MP = min(4, 2) = 2.
        assert_eq!(f.movement, 2);
        // Step 4E: lowest morale = min(7, 8) = 7.
        assert_eq!(f.morale_rating, 7);
        // Step 4: total PV.
        assert_eq!(f.point_value, cu_a.point_value + cu_b.point_value);
        assert!(!f.is_aerospace());
    }

    #[test]
    fn combat_unit_tactics_formula() {
        // Step 3G: base 10 − Move + (skill − 4), floor 0. Move 4, skill 4 → 10 − 4 + 0 = 6.
        let team = convert_combat_team("t", &vec![unit("a", 30, 4.0, 4.0, 4.0, 4, 30); 2]);
        let cu = convert_combat_unit("cu", &[team]);
        assert_eq!(cu.movement, 4);
        assert_eq!(cu.tactics, 6);
    }

    // ---- Phase 3: calculators ----

    #[test]
    fn experience_maps_from_skill() {
        assert_eq!(
            AcsExperience::from_skill(7),
            AcsExperience::WetBehindTheEars
        );
        assert_eq!(AcsExperience::from_skill(4), AcsExperience::Regular);
        assert_eq!(AcsExperience::from_skill(0), AcsExperience::Legendary);
        assert_eq!(AcsExperience::from_skill(-3), AcsExperience::Legendary); // clamps
        assert_eq!(AcsExperience::Regular.to_hit_mod(), -1);
        assert_eq!(AcsExperience::Legendary.morale_mod(), -5);
    }

    #[test]
    fn to_hit_sums_the_master_modifier_rows() {
        // Base 4, Medium +2, Regular −1 = 5, no other mods.
        let base = AcsToHitCtx {
            range: AcsRange::Medium,
            attacker: AcsExperience::Regular,
            ..Default::default()
        };
        assert_eq!(acs_to_hit(&base), 5);

        // Short −1, Green 0, target TMM +3, Aggressive(2) +2, target Broken −3, own Shaken +1,
        // Tired +1, from_behind −1, secondary +2, no_supply +3, misc +1.
        let c = AcsToHitCtx {
            range: AcsRange::Short,
            attacker: AcsExperience::Green,
            target_tmm: 3,
            tactics: AcsTactics::Aggressive(2),
            target_morale: AcsMorale::Broken,
            own_morale: AcsMorale::Shaken,
            fatigue: AcsFatigueBand::Tired,
            from_behind: true,
            secondary_target: true,
            no_supply: true,
            artillery: false,
            misc_mod: 1,
        };
        // 4 -1 +0 +3 +2 -3 +1 +1 -1 +2 +3 +1 = 12.
        assert_eq!(acs_to_hit(&c), 12);
    }

    #[test]
    fn artillery_replaces_the_range_modifier() {
        let c = AcsToHitCtx {
            range: AcsRange::Long, // would be +4
            attacker: AcsExperience::Green,
            artillery: true, // +2 instead
            ..Default::default()
        };
        assert_eq!(acs_to_hit(&c), 6); // 4 + 2
    }

    #[test]
    fn fractional_damage_rounds_normal() {
        // p.249 worked example: 25 short-range × 1.3 = 32.5 → 33. Aggressive(3) on a hit = +0.3.
        let c = AcsDamageCtx {
            attacker_tactics: AcsTactics::Aggressive(3),
            hit: true,
            ..Default::default()
        };
        assert_eq!(acs_damage(25.0, &c), 33);
        // The same aggressive bonus is withheld on a miss (the attack failed): ×1.0 = 25.
        let miss = AcsDamageCtx { hit: false, ..c };
        assert_eq!(acs_damage(25.0, &miss), 25);
        // Secondary target −0.25 and never negative.
        let sec = AcsDamageCtx {
            attacker_tactics: AcsTactics::Standard,
            secondary_target: true,
            ..Default::default()
        };
        assert_eq!(acs_damage(10.0, &sec), 8); // 10 × 0.75 = 7.5 → 8
        let dead = AcsDamageCtx {
            misc_mod: -5.0,
            ..Default::default()
        };
        assert_eq!(acs_damage(10.0, &dead), 0); // floored
    }

    #[test]
    fn damage_band_classifies_by_thresholds() {
        // Armor 20, thresholds 15/10/5.
        let t = [15, 10, 5];
        assert_eq!(acs_damage_band(20, 20, t), AcsDamageBand::NoDamage);
        assert_eq!(acs_damage_band(18, 20, t), AcsDamageBand::NoThreshold);
        assert_eq!(acs_damage_band(15, 20, t), AcsDamageBand::Pct75);
        assert_eq!(acs_damage_band(10, 20, t), AcsDamageBand::Pct50);
        assert_eq!(acs_damage_band(5, 20, t), AcsDamageBand::Pct25);
        assert_eq!(acs_damage_band(0, 20, t), AcsDamageBand::Pct25);
    }

    #[test]
    fn morale_tn_and_failure_table() {
        // TN: morale 6, Veteran −2, Flagging +2, third threshold +2 = 8.
        let c = AcsMoraleCtx {
            morale_value: 6,
            experience: AcsExperience::Veteran,
            fatigue: AcsFatigueBand::Flagging,
            third_threshold: true,
            ..Default::default()
        };
        assert_eq!(acs_morale_tn(&c), 8);

        // A held check (roll ≥ TN → non-positive margin) stays Normal.
        assert_eq!(
            acs_morale_result(AcsDamageBand::Pct25, 0),
            AcsMorale::Normal
        );
        // Failure table corners (p.250): MoF 1–3 at 25% = Broken; MoF 10+ at 25% = Surrender.
        assert_eq!(
            acs_morale_result(AcsDamageBand::Pct25, 2),
            AcsMorale::Broken
        );
        assert_eq!(
            acs_morale_result(AcsDamageBand::Pct25, 11),
            AcsMorale::Surrender
        );
        // MoF 4–6 at 75% = Broken; MoF 7–9 pristine = Unsteady.
        assert_eq!(
            acs_morale_result(AcsDamageBand::Pct75, 5),
            AcsMorale::Broken
        );
        assert_eq!(
            acs_morale_result(AcsDamageBand::NoDamage, 8),
            AcsMorale::Unsteady
        );
    }

    #[test]
    fn aero_matchup_cross_type_rows() {
        use AcsAeroMatchup::*;
        // IO:BF p.241 Master Modifier cross-type rows.
        assert_eq!(AeroVsWarship.to_hit_mod(), -3);
        assert_eq!(AeroVsDropship.to_hit_mod(), -2);
        assert_eq!(DropshipVsAero.to_hit_mod(), 2);
        assert_eq!(DropshipVsWarship.to_hit_mod(), -2);
        assert_eq!(WarshipVsAero.to_hit_mod(), 5);
        assert_eq!(WarshipVsDropship.to_hit_mod(), -1);
        assert_eq!(None.to_hit_mod(), 0);
    }

    #[test]
    fn aero_to_hit_assembly() {
        use crate::engine::large_craft::WeaponClass;
        use crate::engine::sbf::SbfAcm;
        let base = AcsAeroToHitCtx::default();
        // Base 4 + Regular experience(−1) + Medium(+1) = 4.
        assert_eq!(acs_aero_to_hit(&base), 4);
        // WarShip→aero cross-type (+5) + secondary (+1): 4 + 5 + 1 = 10.
        assert_eq!(
            acs_aero_to_hit(&AcsAeroToHitCtx {
                matchup: AcsAeroMatchup::WarshipVsAero,
                secondary_target: true,
                ..base
            }),
            10
        );
        // A CAP shot at Long: capital_range Long→Medium (+1) + CAP +3 (vs a non-large target):
        // 4 − 1 + 1 + 3 = 7.
        let cap = SbfCapital {
            weapon_class: WeaponClass::Cap,
            target_is_large_craft: false,
            high_speed: false,
            atmospheric: false,
            point_defense: 0,
            screen: 0,
            naval_c3: false,
            teleoperated: false,
            crippled: false,
            grappled: false,
            acm: SbfAcm::Off,
        };
        assert_eq!(
            acs_aero_to_hit(&AcsAeroToHitCtx {
                range: SbfRange::Long,
                capital: Some(cap),
                ..base
            }),
            7
        );
        // ACS-specific rows on the base 4: robotic +1, attacked-by-aero +2, target-recon +3,
        // >50% large aero −2 = 4 + 1 + 2 + 3 − 2 = 8.
        assert_eq!(
            acs_aero_to_hit(&AcsAeroToHitCtx {
                robotic: true,
                attacked_by_aero: true,
                target_recon: true,
                over_half_large_aero: true,
                ..base
            }),
            8
        );
    }

    #[test]
    fn ground_support_mission_calculators() {
        // Ground Strike ½ short (p.251); Bomb in 5-pt clusters.
        assert_eq!(acs_ground_strike_damage(10.0), 5);
        assert_eq!(acs_bomb_clusters(12), 3);
        assert_eq!(acs_bomb_clusters(0), 0);
        // Orbit-to-Surface: primary ¼(round up)+1 min 1; secondary ½ primary (round up).
        assert_eq!(acs_orbit_to_surface_primary(10.0), 4);
        assert_eq!(acs_orbit_to_surface_primary(0.0), 1);
        assert_eq!(acs_orbit_to_surface_secondary(4), 2);
        assert_eq!(acs_orbit_to_surface_secondary(3), 2);
    }

    #[test]
    fn combat_drop_results_table() {
        // IO:BF p.251 Combat Drop Results Table, by Margin of Success.
        assert_eq!(acs_combat_drop_result(15).drop_value, 5);
        assert_eq!(acs_combat_drop_result(10).drop_value, 4);
        assert_eq!(acs_combat_drop_result(0).drop_value, 1);
        assert_eq!(acs_combat_drop_result(0).damage_mod, -0.1);
        let bad = acs_combat_drop_result(-8);
        assert_eq!(bad.drop_value, -3);
        assert_eq!(bad.drop_damage_pct, 10);
        let worst = acs_combat_drop_result(-20);
        assert_eq!(worst.drop_value, -5);
        assert_eq!(worst.drop_damage_pct, 20);
    }

    #[test]
    fn fatigue_band_subtracts_experience_ignore() {
        // Regular: no ignore. 5 FP → Tired.
        assert_eq!(
            acs_fatigue_band(5.0, AcsExperience::Regular),
            AcsFatigueBand::Tired
        );
        // Elite ignores the first 2: 6 raw − 2 = 4 → still Rested.
        assert_eq!(
            acs_fatigue_band(6.0, AcsExperience::Elite),
            AcsFatigueBand::Rested
        );
        // 17+ effective = Spent; its columns.
        assert_eq!(
            acs_fatigue_band(17.0, AcsExperience::Regular),
            AcsFatigueBand::Spent
        );
        assert_eq!(AcsFatigueBand::Spent.combat_mod(), 5);
        assert_eq!(AcsFatigueBand::Flagging.damage_mod(), -0.1);
        assert_eq!(AcsExperience::WetBehindTheEars.fatigue_earned(), 2.0);
    }
}
