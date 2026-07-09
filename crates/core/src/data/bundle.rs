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

//! The baked dataset format, shared by the bake tool (encode) and the runtime app (decode).

use crate::domain::Mech;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Bundle format version, bumped on incompatible layout changes.
/// 22: `AsStats.arcs` (large-craft multi-arc card) + DropShip/Small-Craft units baked.
pub const BUNDLE_VERSION: u32 = 22;

/// A BattleTech era (§35), mirroring Mekbay/MUL era ids. Used to resolve a unit's availability
/// scores (keyed by era id) and to map an intro year onto an era for the picker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EraInfo {
    pub id: u16,
    pub name: String,
    pub from: u16,
    pub to: u16,
}

/// A faction (§35), mirroring MUL faction ids — the inner key of a unit's availability table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactionInfo {
    pub id: u16,
    pub name: String,
    /// "Inner Sphere", "Clan", "Periphery", etc. (for grouping in the picker).
    pub group: String,
}

/// A lightweight row for the unit picker (avoids loading full `Mech`s to list them).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MechSummary {
    pub chassis: String,
    pub model: String,
    pub tonnage: u16,
    pub tech_base: String,
    pub role: String,
    pub year: u16,
    pub bv: u32,
}

/// The whole baked dataset: every mech plus a parallel summary index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    pub version: u32,
    pub mechs: Vec<Mech>,
    /// Munition catalog shared across mechs: `baseAmmo` group key ->
    /// the munition display names a bin in that group can load (`"Standard"` first). Referenced
    /// by [`crate::domain::AmmoBin::base_ammo`]. Empty for bundles baked before munition support.
    #[serde(default)]
    pub munitions: BTreeMap<String, Vec<String>>,
    /// BattleTech eras (§35), sorted by `from` year. Resolves the era-id keys of each unit's
    /// `availability` table and maps an intro year to an era. Empty for pre-§35 bundles.
    #[serde(default)]
    pub eras: Vec<EraInfo>,
    /// Factions (§35), the inner key of each unit's `availability` table. Empty for pre-§35 bundles.
    #[serde(default)]
    pub factions: Vec<FactionInfo>,
}

impl Bundle {
    pub fn new(mechs: Vec<Mech>) -> Self {
        Bundle {
            version: BUNDLE_VERSION,
            mechs,
            munitions: BTreeMap::new(),
            eras: Vec::new(),
            factions: Vec::new(),
        }
    }

    /// The era containing `year` (§35), or `None` if no era covers it (e.g. year 0 / unknown).
    pub fn era_for_year(&self, year: u16) -> Option<&EraInfo> {
        self.eras.iter().find(|e| year >= e.from && year <= e.to)
    }

    /// Look up a faction by id.
    pub fn faction(&self, id: u16) -> Option<&FactionInfo> {
        self.factions.iter().find(|f| f.id == id)
    }

    /// The munition options for a bin's `base_ammo` group (empty if unknown / no choice).
    pub fn munitions_for(&self, base_ammo: Option<&str>) -> &[String] {
        base_ammo
            .and_then(|k| self.munitions.get(k))
            .map_or(&[], Vec::as_slice)
    }

    /// Encode to a compact binary blob (bincode).
    pub fn encode(&self) -> Result<Vec<u8>> {
        Ok(bincode::serde::encode_to_vec(
            self,
            bincode::config::standard(),
        )?)
    }

    /// Decode from a binary blob produced by [`Bundle::encode`].
    pub fn decode(bytes: &[u8]) -> Result<Bundle> {
        let (bundle, _) = bincode::serde::decode_from_slice(bytes, bincode::config::standard())?;
        Ok(bundle)
    }

    /// Load and decode a bundle file from disk.
    pub fn load(path: &Path) -> Result<Bundle> {
        let bytes = std::fs::read(path)?;
        Bundle::decode(&bytes)
    }

    /// A picker index over the bundle, in storage order.
    pub fn index(&self) -> Vec<MechSummary> {
        self.mechs
            .iter()
            .map(|m| MechSummary {
                chassis: m.chassis.clone(),
                model: m.model.clone(),
                tonnage: m.tonnage,
                tech_base: m.tech_base.clone(),
                role: m.role.clone(),
                year: m.year,
                bv: m.bv,
            })
            .collect()
    }

    /// Fetch a mech by its index position.
    pub fn get(&self, idx: usize) -> Option<&Mech> {
        self.mechs.get(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AsStats, HeatSinkType, Mech, MechConfig, UnitType};
    use std::collections::BTreeMap;

    fn sample() -> Mech {
        Mech {
            chassis: "Locust".into(),
            model: "LCT-1V".into(),
            tonnage: 20,
            tech_base: "Inner Sphere".into(),
            role: "Scout".into(),
            weight_class: "Light".into(),
            subtype: "BattleMek".into(),
            year: 2499,
            bv: 0,
            cost: 0,
            armor_type: "Standard Armor".into(),
            structure_type: "Standard".into(),
            walk: 8,
            run: 12,
            jump: 0,
            heat_sinks: 10,
            heat_sink_type: HeatSinkType::Single,
            dissipation: 10,
            equipment: Vec::new(),
            config: MechConfig::Biped,
            unit_type: UnitType::Mech,
            motive: None,
            internal: 0,
            dpt: 0,
            transport: vec![],
            armor: BTreeMap::new(),
            weapons: vec![],
            ammo: vec![],
            crit_slots: BTreeMap::new(),
            as_stats: AsStats::default(),
            availability: BTreeMap::new(),
        }
    }

    #[test]
    fn bundle_encode_decode_roundtrip() {
        let bundle = Bundle::new(vec![sample()]);
        let bytes = bundle.encode().unwrap();
        let back = Bundle::decode(&bytes).unwrap();
        assert_eq!(bundle, back);
        assert_eq!(back.version, BUNDLE_VERSION);
        assert_eq!(back.index().len(), 1);
        assert_eq!(back.get(0).unwrap().display_name(), "Locust LCT-1V");
    }
}
