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

//! Heat dissipation and the standard BattleTech heat-effects scale (Total Warfare).

use crate::domain::HeatSinkType;

/// Total heat dissipated per turn by `sinks` heat sinks of the given type.
pub fn dissipation(sinks: u16, kind: HeatSinkType) -> u16 {
    sinks.saturating_mul(kind.per_sink())
}

/// The cumulative effects active at a given heat level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeatEffects {
    /// Movement points lost.
    pub movement_penalty: u8,
    /// To-hit penalty when firing weapons.
    pub to_hit_penalty: u8,
    /// 2d6 target to avoid an automatic shutdown (None = no risk).
    pub shutdown_avoid: Option<u8>,
    /// 2d6 target to avoid an ammo explosion (None = no risk).
    pub ammo_explosion_avoid: Option<u8>,
    /// At/above heat 30 the mech shuts down automatically.
    pub auto_shutdown: bool,
}

/// Highest threshold value `<= heat` from a sorted `(threshold, value)` table, or 0.
fn step(heat: i32, table: &[(i32, u8)]) -> u8 {
    table
        .iter()
        .rev()
        .find(|(t, _)| heat >= *t)
        .map(|(_, v)| *v)
        .unwrap_or(0)
}

fn opt_step(heat: i32, table: &[(i32, u8)]) -> Option<u8> {
    table
        .iter()
        .rev()
        .find(|(t, _)| heat >= *t)
        .map(|(_, v)| *v)
}

/// Resolve all heat effects active at `heat`.
pub fn heat_effects(heat: i32) -> HeatEffects {
    HeatEffects {
        movement_penalty: step(heat, &[(5, 1), (10, 2), (15, 3), (20, 4), (25, 5)]),
        to_hit_penalty: step(heat, &[(8, 1), (13, 2), (17, 3), (24, 4)]),
        shutdown_avoid: opt_step(heat, &[(14, 4), (18, 6), (22, 8), (26, 10)]),
        ammo_explosion_avoid: opt_step(heat, &[(19, 4), (23, 6), (28, 8)]),
        auto_shutdown: heat >= 30,
    }
}

/// Aerospace-fighter heat effects. Differs from the 'Mech scale: heat does **not** reduce thrust —
/// instead it forces a control roll (random movement) — and there's a pilot-damage line. Transcribed
/// from Mekbay `src/app/models/rules/aero-rules.ts` `HEAT_SCALE` (the to-hit / shutdown / ammo values
/// match the 'Mech scale; the control + pilot-damage lines are aero-specific).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AeroHeatEffects {
    /// Weapon to-hit penalty ("fire" line).
    pub to_hit_penalty: u8,
    /// 2d6 target to avoid a control roll / random movement (None = no risk). Aero heat triggers
    /// this rather than cutting thrust.
    pub control_avoid: Option<u8>,
    /// 2d6 target to avoid an automatic shutdown (None = no risk).
    pub shutdown_avoid: Option<u8>,
    /// 2d6 target to avoid an ammo explosion (None = no risk).
    pub ammo_explosion_avoid: Option<u8>,
    /// 2d6 target to avoid pilot damage from heat (None = no risk).
    pub pilot_damage_avoid: Option<u8>,
    /// At/above heat 30 the fighter shuts down automatically.
    pub auto_shutdown: bool,
}

/// Resolve the aerospace heat effects active at `heat`.
pub fn aero_heat_effects(heat: i32) -> AeroHeatEffects {
    AeroHeatEffects {
        to_hit_penalty: step(heat, &[(8, 1), (13, 2), (17, 3), (24, 4)]),
        control_avoid: opt_step(heat, &[(5, 5), (10, 6), (15, 7), (20, 8), (25, 10)]),
        shutdown_avoid: opt_step(heat, &[(14, 4), (18, 6), (22, 8), (26, 10)]),
        ammo_explosion_avoid: opt_step(heat, &[(19, 4), (23, 6), (28, 8)]),
        pilot_damage_avoid: opt_step(heat, &[(21, 6), (27, 9)]),
        auto_shutdown: heat >= 30,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dissipation_doubles_for_double_sinks() {
        assert_eq!(dissipation(10, HeatSinkType::Single), 10);
        assert_eq!(dissipation(15, HeatSinkType::Double), 30);
    }

    #[test]
    fn cool_mech_has_no_effects() {
        assert_eq!(heat_effects(0), HeatEffects::default());
        assert_eq!(heat_effects(4), HeatEffects::default());
    }

    #[test]
    fn thresholds_are_cumulative_worst() {
        let e = heat_effects(5);
        assert_eq!(e.movement_penalty, 1);
        assert_eq!(e.to_hit_penalty, 0);

        let e = heat_effects(14);
        assert_eq!(e.movement_penalty, 2); // >=10
        assert_eq!(e.to_hit_penalty, 2); // >=13
        assert_eq!(e.shutdown_avoid, Some(4)); // >=14
        assert_eq!(e.ammo_explosion_avoid, None); // <19

        let e = heat_effects(19);
        assert_eq!(e.ammo_explosion_avoid, Some(4));
        assert_eq!(e.shutdown_avoid, Some(6)); // >=18

        let e = heat_effects(30);
        assert!(e.auto_shutdown);
        assert_eq!(e.movement_penalty, 5);
        assert_eq!(e.to_hit_penalty, 4);
        assert_eq!(e.shutdown_avoid, Some(10));
        assert_eq!(e.ammo_explosion_avoid, Some(8));
    }

    #[test]
    fn aero_heat_scale_differs_from_mech() {
        // No thrust loss — heat 5 is a control roll, not a movement penalty.
        let e = aero_heat_effects(5);
        assert_eq!(e.control_avoid, Some(5));
        assert_eq!(e.to_hit_penalty, 0);
        // Pilot-damage line is aero-specific.
        assert_eq!(aero_heat_effects(20).pilot_damage_avoid, None);
        assert_eq!(aero_heat_effects(21).pilot_damage_avoid, Some(6));
        assert_eq!(aero_heat_effects(27).pilot_damage_avoid, Some(9));
        // To-hit / shutdown / ammo match the 'Mech values.
        let e = aero_heat_effects(24);
        assert_eq!(e.to_hit_penalty, 4);
        assert_eq!(e.control_avoid, Some(8)); // >=20
        assert_eq!(e.shutdown_avoid, Some(8)); // >=22
        assert_eq!(e.ammo_explosion_avoid, Some(6)); // >=23
        assert!(aero_heat_effects(30).auto_shutdown);
    }
}
