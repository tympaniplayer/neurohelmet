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

//! MekBay-style faceted filtering for the unit picker. Facets compose with the fuzzy text search:
//! a unit passes when it matches every *set* facet (an unset facet = "any"). Each facet holds a
//! single value here (cycled with ←→), defaulting to "(any)".

use neurohelmet_core::data::bundle::Bundle;
use neurohelmet_core::domain::{Mech, UnitType};

/// The filterable facets, in display order.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Facet {
    Type,
    Tech,
    Class,
    Role,
    Era,
    /// Earliest intro year of the range (typed; inclusive lower bound).
    YearMin,
    /// Latest intro year of the range (typed; inclusive upper bound).
    YearMax,
    Family,
    /// Availability lens (§35): the era at which to evaluate faction availability. Soft — it tints
    /// and sorts the list by rarity, it does **not** hide units. Distinct from `Era` (intro era).
    AvailEra,
    /// Availability lens (§35): the faction whose availability tints/sorts the list.
    Faction,
}

impl Facet {
    pub const ALL: [Facet; 10] = [
        Facet::Type,
        Facet::Tech,
        Facet::Class,
        Facet::Role,
        Facet::Era,
        Facet::YearMin,
        Facet::YearMax,
        Facet::Family,
        Facet::AvailEra,
        Facet::Faction,
    ];

    /// Left-column label shown in the filter modal.
    pub fn label(self) -> &'static str {
        match self {
            Facet::Type => "Type",
            Facet::Tech => "Tech",
            Facet::Class => "Class",
            Facet::Role => "Role",
            Facet::Era => "Era",
            Facet::YearMin => "Year ≥",
            Facet::YearMax => "Year ≤",
            Facet::Family => "Family",
            Facet::AvailEra => "Avail era",
            Facet::Faction => "Faction",
        }
    }

    /// Whether this facet is part of the §35 availability lens (tints/sorts, never hides).
    pub fn is_avail(self) -> bool {
        matches!(self, Facet::AvailEra | Facet::Faction)
    }

    /// Whether this is one of the typed year-range bounds (edited with digits, not cycled).
    pub fn is_year(self) -> bool {
        matches!(self, Facet::YearMin | Facet::YearMax)
    }
}

/// Type-facet values: the five coarse [`UnitType`]s plus two refined 'Mech chassis picks, keyed
/// off the baked Mekbay `subtype` ("BattleMek" / "BattleMek Omni" / "Industrial Mek" / …).
/// `BattleMech` spans both tech bases (Tech is its own facet) and includes OmniMechs;
/// `OmniMech` narrows to omni chassis. Both exclude IndustrialMechs — the coarse `Mech` keeps them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TypeFilter {
    Unit(UnitType),
    BattleMech,
    OmniMech,
}

/// Unit-type facet values (fixed, in cycle order) and their display labels.
const TYPE_VALUES: [TypeFilter; 7] = [
    TypeFilter::Unit(UnitType::Mech),
    TypeFilter::BattleMech,
    TypeFilter::OmniMech,
    TypeFilter::Unit(UnitType::Vehicle),
    TypeFilter::Unit(UnitType::Infantry),
    TypeFilter::Unit(UnitType::BattleArmor),
    TypeFilter::Unit(UnitType::Aerospace),
];

fn type_filter_label(t: TypeFilter) -> &'static str {
    match t {
        TypeFilter::Unit(UnitType::Mech) => "Mech",
        TypeFilter::BattleMech => "BattleMech",
        TypeFilter::OmniMech => "OmniMech",
        TypeFilter::Unit(UnitType::Vehicle) => "Vehicle",
        TypeFilter::Unit(UnitType::Infantry) => "Infantry",
        TypeFilter::Unit(UnitType::BattleArmor) => "Battle Armor",
        TypeFilter::Unit(UnitType::Aerospace) => "Aerospace",
    }
}

impl TypeFilter {
    /// Whether a unit passes this type pick.
    fn matches(self, m: &Mech) -> bool {
        match self {
            TypeFilter::Unit(u) => m.unit_type == u,
            TypeFilter::BattleMech => m.subtype == "BattleMek" || m.subtype == "BattleMek Omni",
            TypeFilter::OmniMech => m.subtype == "BattleMek Omni",
        }
    }
}

/// BattleTech eras, by introduction year (fixed facet values). A `year == 0` (unknown) unit
/// belongs to no era and is excluded when an Era filter is active.
pub const ERAS: [&str; 8] = [
    "Age of War",
    "Star League",
    "Succession Wars",
    "Clan Invasion",
    "Civil War",
    "Jihad",
    "Dark Age",
    "ilClan",
];

/// The era a unit's introduction year falls in (`None` for an unknown/0 year).
pub fn era_for_year(year: u16) -> Option<&'static str> {
    Some(match year {
        0 => return None,
        1..=2570 => "Age of War",
        2571..=2780 => "Star League",
        2781..=3049 => "Succession Wars",
        3050..=3061 => "Clan Invasion",
        3062..=3067 => "Civil War",
        3068..=3080 => "Jihad",
        3081..=3150 => "Dark Age",
        _ => "ilClan",
    })
}

/// The active filter selection — one value per facet (`None` = any).
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Filters {
    pub unit_type: Option<TypeFilter>,
    pub tech: Option<String>,
    pub class: Option<String>,
    pub role: Option<String>,
    pub era: Option<&'static str>,
    /// Inclusive intro-year range (typed bounds, either side optional). Independent of `era`; all
    /// set facets AND together.
    pub year_min: Option<u16>,
    pub year_max: Option<u16>,
    pub family: Option<String>,
    /// §35 availability lens: `(era_id, name)` at which to evaluate faction availability. Soft —
    /// drives the rarity tint + sort, never hides a unit (so `matches` ignores it).
    pub avail_era: Option<(u16, String)>,
    /// §35 availability lens: `(faction_id, name)`. Soft, like `avail_era`.
    pub faction: Option<(u16, String)>,
}

impl Filters {
    pub fn is_empty(&self) -> bool {
        *self == Filters::default()
    }

    pub fn clear(&mut self) {
        *self = Filters::default();
    }

    /// The current value of a facet for display: the value, or `"(any)"` when unset.
    pub fn value_label(&self, facet: Facet) -> String {
        match facet {
            Facet::Type => self
                .unit_type
                .map(type_filter_label)
                .unwrap_or("(any)")
                .to_string(),
            Facet::Tech => self.tech.clone().unwrap_or_else(|| "(any)".into()),
            Facet::Class => self.class.clone().unwrap_or_else(|| "(any)".into()),
            Facet::Role => self.role.clone().unwrap_or_else(|| "(any)".into()),
            Facet::Era => self.era.unwrap_or("(any)").to_string(),
            Facet::YearMin => self
                .year_min
                .map_or_else(|| "(any)".into(), |y| y.to_string()),
            Facet::YearMax => self
                .year_max
                .map_or_else(|| "(any)".into(), |y| y.to_string()),
            Facet::Family => self.family.clone().unwrap_or_else(|| "(any)".into()),
            Facet::AvailEra => self
                .avail_era
                .as_ref()
                .map_or_else(|| "(any)".into(), |(_, n)| n.clone()),
            Facet::Faction => self
                .faction
                .as_ref()
                .map_or_else(|| "(any)".into(), |(_, n)| n.clone()),
        }
    }

    /// The active §35 availability lens as `(era_id?, faction_id?)`, or `None` when neither the
    /// `Faction` nor `AvailEra` facet is set (so the picker renders/sorts normally).
    pub fn avail_context(&self) -> Option<(Option<u16>, Option<u16>)> {
        if self.faction.is_none() && self.avail_era.is_none() {
            return None;
        }
        Some((
            self.avail_era.as_ref().map(|(id, _)| *id),
            self.faction.as_ref().map(|(id, _)| *id),
        ))
    }

    /// The mutable year bound a year facet edits (`None` for non-year facets).
    fn year_slot(&mut self, facet: Facet) -> Option<&mut Option<u16>> {
        match facet {
            Facet::YearMin => Some(&mut self.year_min),
            Facet::YearMax => Some(&mut self.year_max),
            _ => None,
        }
    }

    /// Append a typed digit to a year bound (`3` `0` `5` `0` → 3050), capped at four digits. The
    /// `* 10` is done in `u32` so a full 4-digit value plus another keystroke can't overflow `u16`.
    pub fn year_push_digit(&mut self, facet: Facet, d: char) {
        if let (Some(slot), Some(digit)) = (self.year_slot(facet), d.to_digit(10)) {
            let next = u32::from(slot.unwrap_or(0)) * 10 + digit;
            *slot = Some(next.min(9999) as u16);
        }
    }

    /// Delete the last typed digit of a year bound (→ `None` when empty).
    pub fn year_backspace(&mut self, facet: Facet) {
        if let Some(slot) = self.year_slot(facet) {
            *slot = slot.map(|y| y / 10).filter(|&y| y > 0);
        }
    }

    /// Nudge a set year bound by `dir` (±1); a no-op when that bound is empty.
    pub fn year_step(&mut self, facet: Facet, dir: i32) {
        if let Some(slot) = self.year_slot(facet) {
            if let Some(y) = *slot {
                let n = (i32::from(y) + dir).clamp(0, 9999);
                *slot = (n > 0).then_some(n as u16);
            }
        }
    }

    /// A compact one-line summary of the set facets, e.g. `"Mech · Clan · Heavy · 3050–3067"`
    /// (empty when none). The year range collapses to one chip (`3050+`, `≤3067`, `3050–3067`).
    pub fn summary(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(t) = self.unit_type {
            parts.push(type_filter_label(t).to_string());
        }
        for v in [&self.tech, &self.class, &self.role].into_iter().flatten() {
            parts.push(v.clone());
        }
        if let Some(e) = self.era {
            parts.push(e.to_string());
        }
        if let Some(y) = self.year_summary() {
            parts.push(y);
        }
        if let Some(f) = &self.family {
            parts.push(f.clone());
        }
        // The availability lens reads as "@ <faction> in <era>" so it's clearly distinct from the
        // hard intro filters above.
        if let Some(lens) = self.avail_lens_summary() {
            parts.push(lens);
        }
        parts.join(" · ")
    }

    /// The availability-lens chip for the summary, e.g. `"@ Draconis Combine"` / `"@ Clan Invasion"`
    /// / `"@ Draconis Combine in Clan Invasion"` (`None` when the lens is off).
    fn avail_lens_summary(&self) -> Option<String> {
        match (&self.faction, &self.avail_era) {
            (None, None) => None,
            (Some((_, f)), None) => Some(format!("@ {f}")),
            (None, Some((_, e))) => Some(format!("@ {e}")),
            (Some((_, f)), Some((_, e))) => Some(format!("@ {f} in {e}")),
        }
    }

    /// The year-range chip for the summary, e.g. `"3050+"` / `"≤3067"` / `"3050–3067"`.
    fn year_summary(&self) -> Option<String> {
        match (self.year_min, self.year_max) {
            (None, None) => None,
            (Some(a), None) => Some(format!("{a}+")),
            (None, Some(b)) => Some(format!("≤{b}")),
            (Some(a), Some(b)) => Some(format!("{a}–{b}")),
        }
    }
}

/// Whether a unit passes the active filters (every set *hard* facet matches). The §35 availability
/// lens (`faction`/`avail_era`) is deliberately excluded — it tints and sorts but never hides.
pub fn matches(f: &Filters, m: &Mech) -> bool {
    if let Some(t) = f.unit_type {
        if !t.matches(m) {
            return false;
        }
    }
    if let Some(t) = &f.tech {
        if &m.tech_base != t {
            return false;
        }
    }
    if let Some(c) = &f.class {
        if &m.weight_class != c {
            return false;
        }
    }
    if let Some(r) = &f.role {
        if &m.role != r {
            return false;
        }
    }
    if let Some(e) = f.era {
        if era_for_year(m.year) != Some(e) {
            return false;
        }
    }
    if let Some(lo) = f.year_min {
        if m.year < lo {
            return false;
        }
    }
    if let Some(hi) = f.year_max {
        if m.year > hi {
            return false;
        }
    }
    if let Some(fam) = &f.family {
        if &m.subtype != fam {
            return false;
        }
    }
    true
}

/// The cyclable value lists for the data-derived facets (distinct values present in the bundle,
/// most-common first). Type and Era are fixed, so they aren't stored here.
pub struct FacetValues {
    pub tech: Vec<String>,
    pub class: Vec<String>,
    pub role: Vec<String>,
    pub family: Vec<String>,
    /// §35 availability lens values, `(id, name)`. Eras in chronological order; factions grouped
    /// (Inner Sphere / Clan / …) then alphabetical, matching the bundle's stored order.
    pub eras: Vec<(u16, String)>,
    pub factions: Vec<(u16, String)>,
}

impl FacetValues {
    pub fn from_bundle(b: &Bundle) -> Self {
        FacetValues {
            tech: distinct_by_freq(b.mechs.iter().map(|m| m.tech_base.as_str())),
            class: distinct_by_freq(b.mechs.iter().map(|m| m.weight_class.as_str())),
            role: distinct_by_freq(b.mechs.iter().map(|m| m.role.as_str())),
            family: distinct_by_freq(b.mechs.iter().map(|m| m.subtype.as_str())),
            eras: b.eras.iter().map(|e| (e.id, e.name.clone())).collect(),
            factions: b.factions.iter().map(|f| (f.id, f.name.clone())).collect(),
        }
    }
}

/// Distinct non-empty values, ordered by descending frequency (ties broken alphabetically).
fn distinct_by_freq<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    use std::collections::HashMap;
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for v in values {
        if !v.is_empty() {
            *counts.entry(v).or_insert(0) += 1;
        }
    }
    let mut out: Vec<(&str, usize)> = counts.into_iter().collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    out.into_iter().map(|(v, _)| v.to_string()).collect()
}

/// Advance a facet's selection through `[(any), …values]` by `dir` (+1/−1), wrapping.
pub fn cycle(f: &mut Filters, facet: Facet, fv: &FacetValues, dir: i32) {
    match facet {
        Facet::Type => f.unit_type = cycle_opt(&f.unit_type, &TYPE_VALUES, dir),
        Facet::Tech => f.tech = cycle_opt(&f.tech, &fv.tech, dir),
        Facet::Class => f.class = cycle_opt(&f.class, &fv.class, dir),
        Facet::Role => f.role = cycle_opt(&f.role, &fv.role, dir),
        Facet::Era => f.era = cycle_opt(&f.era, &ERAS, dir),
        // Year bounds are typed, not cycled; ←→ just nudges a set bound by ±1.
        Facet::YearMin | Facet::YearMax => f.year_step(facet, dir),
        Facet::Family => f.family = cycle_opt(&f.family, &fv.family, dir),
        Facet::AvailEra => f.avail_era = cycle_opt(&f.avail_era, &fv.eras, dir),
        Facet::Faction => f.faction = cycle_opt(&f.faction, &fv.factions, dir),
    }
}

/// Cycle an `Option<T>` over a value list where index 0 is `None` ("any") and 1..=len map to
/// `values`. Wraps in both directions. (Shared with the §35 force-generator modal.)
pub fn cycle_opt<T: Clone + PartialEq>(cur: &Option<T>, values: &[T], dir: i32) -> Option<T> {
    if values.is_empty() {
        return None;
    }
    let len = values.len() as i32;
    let cur_idx = match cur {
        None => 0,
        Some(v) => values
            .iter()
            .position(|x| x == v)
            .map_or(0, |p| p as i32 + 1),
    };
    let next = (cur_idx + dir).rem_euclid(len + 1);
    if next == 0 {
        None
    } else {
        Some(values[(next - 1) as usize].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn era_boundaries() {
        assert_eq!(era_for_year(0), None);
        assert_eq!(era_for_year(2570), Some("Age of War"));
        assert_eq!(era_for_year(2571), Some("Star League"));
        assert_eq!(era_for_year(2780), Some("Star League"));
        assert_eq!(era_for_year(2781), Some("Succession Wars"));
        assert_eq!(era_for_year(3049), Some("Succession Wars"));
        assert_eq!(era_for_year(3050), Some("Clan Invasion"));
        assert_eq!(era_for_year(3150), Some("Dark Age"));
        assert_eq!(era_for_year(3151), Some("ilClan"));
    }

    fn mech(tech: &str, class: &str, year: u16, ut: UnitType) -> Mech {
        Mech {
            tech_base: tech.into(),
            weight_class: class.into(),
            year,
            unit_type: ut,
            ..Default::default()
        }
    }

    #[test]
    fn matches_ands_across_facets() {
        let clan_heavy = mech("Clan", "Heavy", 3055, UnitType::Mech);
        let is_light = mech("Inner Sphere", "Light", 3025, UnitType::Mech);

        // Empty filter passes everything.
        assert!(matches(&Filters::default(), &clan_heavy));

        // Single facet.
        let mut f = Filters {
            tech: Some("Clan".into()),
            ..Default::default()
        };
        assert!(matches(&f, &clan_heavy));
        assert!(!matches(&f, &is_light));

        // AND across facets: Clan + Heavy.
        f.class = Some("Heavy".into());
        assert!(matches(&f, &clan_heavy));
        f.class = Some("Assault".into());
        assert!(!matches(&f, &clan_heavy));

        // Era excludes unknown-year units.
        let unknown = mech("Clan", "Heavy", 0, UnitType::Mech);
        let f = Filters {
            era: Some("Clan Invasion"),
            ..Default::default()
        };
        assert!(matches(&f, &clan_heavy)); // 3055
        assert!(!matches(&f, &unknown));
    }

    #[test]
    fn battlemech_and_omnimech_type_picks() {
        let mut is_bm = mech("Inner Sphere", "Assault", 3025, UnitType::Mech);
        is_bm.subtype = "BattleMek".into();
        let mut clan_omni = mech("Clan", "Heavy", 3055, UnitType::Mech);
        clan_omni.subtype = "BattleMek Omni".into();
        let mut industrial = mech("Inner Sphere", "Medium", 3040, UnitType::Mech);
        industrial.subtype = "Industrial Mek".into();
        let mut tank = mech("Inner Sphere", "Heavy", 3025, UnitType::Vehicle);
        tank.subtype = "Combat Vehicle".into();

        // BattleMech: both tech bases, standard and omni chassis alike; no Industrials/vehicles.
        let f = Filters {
            unit_type: Some(TypeFilter::BattleMech),
            ..Default::default()
        };
        assert!(matches(&f, &is_bm));
        assert!(matches(&f, &clan_omni));
        assert!(!matches(&f, &industrial));
        assert!(!matches(&f, &tank));

        // OmniMech: omni chassis only.
        let f = Filters {
            unit_type: Some(TypeFilter::OmniMech),
            ..Default::default()
        };
        assert!(!matches(&f, &is_bm));
        assert!(matches(&f, &clan_omni));
        assert!(!matches(&f, &industrial));

        // The coarse Mech pick keeps IndustrialMechs (unchanged behavior).
        let f = Filters {
            unit_type: Some(TypeFilter::Unit(UnitType::Mech)),
            ..Default::default()
        };
        assert!(matches(&f, &is_bm) && matches(&f, &clan_omni) && matches(&f, &industrial));
        assert!(!matches(&f, &tank));

        // Cycle order surfaces the new picks right after Mech.
        let fv = FacetValues {
            tech: vec![],
            class: vec![],
            role: vec![],
            family: vec![],
            eras: vec![],
            factions: vec![],
        };
        let mut f = Filters::default();
        cycle(&mut f, Facet::Type, &fv, 1);
        assert_eq!(f.value_label(Facet::Type), "Mech");
        cycle(&mut f, Facet::Type, &fv, 1);
        assert_eq!(f.value_label(Facet::Type), "BattleMech");
        cycle(&mut f, Facet::Type, &fv, 1);
        assert_eq!(f.value_label(Facet::Type), "OmniMech");
        assert_eq!(f.summary(), "OmniMech");
    }

    #[test]
    fn year_range_filter() {
        let y2750 = mech("Inner Sphere", "Assault", 2750, UnitType::Mech);
        let y3055 = mech("Clan", "Heavy", 3055, UnitType::Mech);
        let y3140 = mech("Inner Sphere", "Light", 3140, UnitType::Mech);

        // Type a lower bound "3000" on the Year≥ facet, one digit at a time.
        let mut f = Filters::default();
        for d in "3000".chars() {
            f.year_push_digit(Facet::YearMin, d);
        }
        assert_eq!(f.year_min, Some(3000));
        assert!(!matches(&f, &y2750)); // below the floor
        assert!(matches(&f, &y3055));
        assert!(matches(&f, &y3140));

        // Add an upper bound "3100" → a 3000–3100 window.
        for d in "3100".chars() {
            f.year_push_digit(Facet::YearMax, d);
        }
        assert_eq!((f.year_min, f.year_max), (Some(3000), Some(3100)));
        assert!(matches(&f, &y3055));
        assert!(!matches(&f, &y3140)); // above the ceiling
        assert_eq!(f.year_summary().as_deref(), Some("3000–3100"));

        // ←→ nudges a bound; backspace deletes a digit; deleting all → any.
        f.year_step(Facet::YearMax, -1);
        assert_eq!(f.year_max, Some(3099));
        f.year_backspace(Facet::YearMin);
        assert_eq!(f.year_min, Some(300));
        // No overflow when pushing past four digits on a maxed-out bound (regression).
        f.year_min = Some(9999);
        f.year_push_digit(Facet::YearMin, '9');
        assert_eq!(f.year_min, Some(9999));
    }

    #[test]
    fn availability_lens_is_soft_and_sorts() {
        use crate::tui::picker::Picker;
        use neurohelmet_core::data::bundle::Bundle;
        use std::collections::BTreeMap;

        // Three 'Mechs, all Inner Sphere so the hard facets don't interfere. Faction 27 in era 13:
        // common(70), rare(15); the third has no RAT data at all.
        let mut common = mech("Inner Sphere", "Heavy", 3055, UnitType::Mech);
        common.chassis = "Common".into();
        common.availability = BTreeMap::from([(13u16, BTreeMap::from([(27u16, 70u8)]))]);
        let mut rare = mech("Inner Sphere", "Heavy", 3055, UnitType::Mech);
        rare.chassis = "Rare".into();
        rare.availability = BTreeMap::from([(13u16, BTreeMap::from([(27u16, 15u8)]))]);
        let mut nodata = mech("Inner Sphere", "Heavy", 3055, UnitType::Mech);
        nodata.chassis = "NoData".into();

        // Bundle order: rare, nodata, common (so a no-op sort would NOT already be rarity order).
        let bundle = Bundle::new(vec![rare.clone(), nodata.clone(), common.clone()]);
        let names: Vec<String> = bundle.mechs.iter().map(|m| m.display_name()).collect();

        // The lens is soft: even though `nodata` is unavailable to faction 27, it still passes.
        let f = Filters {
            faction: Some((27, "Draconis Combine".into())),
            avail_era: Some((13, "Clan Invasion".into())),
            ..Default::default()
        };
        assert!(matches(&f, &nodata), "lens must never hide a unit");
        assert!(f.avail_context().is_some());
        assert!(Filters::default().avail_context().is_none());

        // refilter sorts most-available first; unknown sorts last.
        let mut p = Picker::new(bundle.mechs.len());
        p.refilter(&names, &bundle, &f);
        let order: Vec<&str> = p
            .filtered
            .iter()
            .map(|&i| bundle.mechs[i].chassis.as_str())
            .collect();
        assert_eq!(order, vec!["Common", "Rare", "NoData"]);

        // With no lens, order is bundle order (no rarity sort).
        let mut p2 = Picker::new(bundle.mechs.len());
        p2.refilter(&names, &bundle, &Filters::default());
        let order2: Vec<&str> = p2
            .filtered
            .iter()
            .map(|&i| bundle.mechs[i].chassis.as_str())
            .collect();
        assert_eq!(order2, vec!["Rare", "NoData", "Common"]);
    }

    #[test]
    fn picker_page_jump_clamps() {
        use crate::tui::picker::Picker;
        let mut p = Picker::new(30); // filtered = 0..30
        p.page = 10;
        p.page_jump(1);
        assert_eq!(p.selected, 10);
        p.page_jump(1);
        assert_eq!(p.selected, 20);
        p.page_jump(1);
        assert_eq!(p.selected, 29); // clamps at the last row, no wrap
        p.page_jump(-1);
        assert_eq!(p.selected, 19);
        p.page_jump(-1);
        p.page_jump(-1);
        assert_eq!(p.selected, 0); // clamps at the top
                                   // Empty list is a no-op.
        let mut empty = Picker::new(0);
        empty.page_jump(1);
        assert_eq!(empty.selected, 0);
    }

    #[test]
    fn cycle_wraps_through_any() {
        let fv = FacetValues {
            tech: vec!["Inner Sphere".into(), "Clan".into()],
            class: vec![],
            role: vec![],
            family: vec![],
            eras: vec![],
            factions: vec![],
        };
        let mut f = Filters::default();
        cycle(&mut f, Facet::Tech, &fv, 1); // any -> first
        assert_eq!(f.tech.as_deref(), Some("Inner Sphere"));
        cycle(&mut f, Facet::Tech, &fv, 1); // -> second
        assert_eq!(f.tech.as_deref(), Some("Clan"));
        cycle(&mut f, Facet::Tech, &fv, 1); // -> back to any
        assert_eq!(f.tech, None);
        cycle(&mut f, Facet::Tech, &fv, -1); // any -> last (wrap backwards)
        assert_eq!(f.tech.as_deref(), Some("Clan"));
    }
}
