//! Shared large-craft (DropShip→WarShip) combat helpers: the multi-arc AS/BF card, arc /
//! weapon-class damage selection, and the damage-threshold gate. Consumed by BattleForce now, and
//! by SBF/ACS as their capital layers land. See `docs/large-craft-implementation-spec.md`.
//!
//! Doctrine reminder: the arc *values* are already final BF-scale — the capital classes
//! (CAP/SCAP/MSL) drive the to-hit weapon-class modifier and the crit weapon-class pick, NOT a
//! damage rescale.

use super::as_element::{as_damage, DamageVector};
use crate::domain::{ArcCard, ArcDamage, FiringArc};

/// AS/BF weapon classes. `Std` is standard-scale; `Cap`/`ScAp`/`Msl` are capital, sub-capital, and
/// capital/sub-capital missiles. Small Craft carry only `Std`; DropShips add `ScAp`/`Msl`; capital
/// craft (Phase 2) add `Cap`. Order fixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WeaponClass {
    #[default]
    Std,
    Cap,
    ScAp,
    Msl,
}

impl WeaponClass {
    pub const ALL: [WeaponClass; 4] = [Self::Std, Self::Cap, Self::ScAp, Self::Msl];

    pub fn label(self) -> &'static str {
        match self {
            Self::Std => "STD",
            Self::Cap => "CAP",
            Self::ScAp => "SCAP",
            Self::Msl => "MSL",
        }
    }

    /// Capital-scale weapon-class to-hit modifier for **Advanced Strategic Aerospace** (the SBF /
    /// Capital-Scale Aerospace Combat subsystem, IO:BF p.191): CAP +3, SCAP +2, MSL/STD +0, waived
    /// vs large-craft targets. This is the SBF table (reserved for the SBF phase) — standard
    /// BattleForce uses [`WeaponClass::bf_vs_small_mod`] instead (the p.83 Advanced Combat
    /// Modifiers Table, a different subsystem with different values).
    pub fn to_hit_mod(self) -> i32 {
        match self {
            Self::Cap => 3,
            Self::ScAp => 2,
            Self::Std | Self::Msl => 0,
        }
    }

    /// Standard-BattleForce "Capital/Sub-Capital Weapon vs. Small Target" to-hit modifier
    /// (IO:BF p.83 Advanced Combat Modifiers Table, footnote 28): a capital non-missile (CAP)
    /// attack takes **+5** and a sub-capital non-missile (SCAP) attack **+3** *only* when the
    /// target is a small aerospace unit — an aerospace/conventional fighter, fighter squadron,
    /// Small Craft or Satellite. It does not apply to capital missiles, to standard weapons, or vs
    /// large-craft / ground targets (`target_is_small_aero == false` → +0). This is the BF table;
    /// the capital-scale p.191 table ([`to_hit_mod`]) is a separate SBF subsystem.
    pub fn bf_vs_small_mod(self, target_is_small_aero: bool) -> i32 {
        if !target_is_small_aero {
            return 0;
        }
        match self {
            Self::Cap => 5,
            Self::ScAp => 3,
            Self::Std | Self::Msl => 0,
        }
    }

    pub fn is_capital(self) -> bool {
        matches!(self, Self::Cap | Self::ScAp | Self::Msl)
    }

    /// This class's raw damage strings within a firing arc.
    pub fn of(self, arc: &FiringArc) -> &ArcDamage {
        match self {
            Self::Std => &arc.std,
            Self::Cap => &arc.cap,
            Self::ScAp => &arc.scap,
            Self::Msl => &arc.msl,
        }
    }
}

/// The four AS/BF firing arcs. `Nose`/`Aft` are the card's front/rear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Arc {
    #[default]
    Nose,
    Left,
    Right,
    Aft,
}

impl Arc {
    pub const ALL: [Arc; 4] = [Self::Nose, Self::Left, Self::Right, Self::Aft];

    pub fn label(self) -> &'static str {
        match self {
            Self::Nose => "Nose",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Aft => "Aft",
        }
    }

    /// This arc's per-class damage + specials on the card.
    pub fn of(self, card: &ArcCard) -> &FiringArc {
        match self {
            Self::Nose => &card.front,
            Self::Left => &card.left,
            Self::Right => &card.right,
            Self::Aft => &card.rear,
        }
    }
}

/// Parse one arc/class's S/M/L/E damage into a typed vector (`"0*"` → minimal `0.5`, `"-"`/`""` → 0).
pub fn arc_damage(card: &ArcCard, arc: Arc, class: WeaponClass) -> DamageVector {
    let ad = class.of(arc.of(card));
    DamageVector {
        s: as_damage(&ad.s),
        m: as_damage(&ad.m),
        l: Some(as_damage(&ad.l)),
        e: Some(as_damage(&ad.e)),
    }
}

/// Whether a damage vector is entirely zero across all bands (a minimal `0.5` counts as non-zero).
pub fn is_zero(d: &DamageVector) -> bool {
    d.s == 0.0 && d.m == 0.0 && d.l.unwrap_or(0.0) == 0.0 && d.e.unwrap_or(0.0) == 0.0
}

/// The weapon-class lines an arc actually carries, in `WeaponClass::ALL` order, skipping all-zero
/// classes. For card rendering and the shot-builder's class picker.
pub fn arc_lines(card: &ArcCard, arc: Arc) -> Vec<(WeaponClass, DamageVector)> {
    WeaponClass::ALL
        .into_iter()
        .map(|c| (c, arc_damage(card, arc, c)))
        .filter(|(_, d)| !is_zero(d))
        .collect()
}

/// Whether a single attack's damage meets the card's damage Threshold (triggers a crit roll — it
/// does NOT bypass armor; IO:BF p.40). A `0`/absent threshold never triggers; minimal `0.5` damage
/// can never meet an integer threshold ≥ 1. Standard-BattleForce (Element-scale) only — SBF uses a
/// below-half-armor crit gate instead (there is no Threshold at Strategic scale).
pub fn threshold_triggered(single_attack_dmg: f32, threshold: u8) -> bool {
    threshold > 0 && single_attack_dmg >= threshold as f32
}

/// The Random Weapon Class pick (IO:BF p.190, 1D6): 1-2 Standard, 3-4 Capital non-missile, 5-6
/// Capital/sub-capital missile. Used on an SBF "Weapon Damage" critical against a Flight that
/// carries more than one weapon class in the struck arc, to pick which class the crit knocks out.
/// The table has only three buckets — a sub-capital non-missile (`ScAp`) falls in the 3-4 "Capital
/// non-missile" bucket (it is broken out only on the to-hit table). Returns `None` for a roll
/// outside 1..=6.
pub fn random_weapon_class(d6: u8) -> Option<WeaponClass> {
    match d6 {
        1 | 2 => Some(WeaponClass::Std),
        3 | 4 => Some(WeaponClass::Cap),
        5 | 6 => Some(WeaponClass::Msl),
        _ => None,
    }
}

/// The printed form of one class's damage in an arc (`"4/3/2/0*"`), preserving `0*` and rendering
/// empty/`"-"` bands as `"0"`. Returns `None` for a fully-zero line (nothing to display). Unlike the
/// parsed [`arc_damage`], this keeps the source strings for faithful card rendering.
pub fn arc_class_display(card: &ArcCard, arc: Arc, class: WeaponClass) -> Option<String> {
    let ad = class.of(arc.of(card));
    let bands = [&ad.s, &ad.m, &ad.l, &ad.e];
    if bands.iter().all(|s| matches!(s.trim(), "" | "0" | "-")) {
        return None;
    }
    fn band(s: &str) -> &str {
        let t = s.trim();
        if t.is_empty() { "0" } else { t }
    }
    Some(format!("{}/{}/{}/{}", band(&ad.s), band(&ad.m), band(&ad.l), band(&ad.e)))
}

/// The non-empty printed class lines for an arc, in class order — for card rendering.
pub fn arc_display_lines(card: &ArcCard, arc: Arc) -> Vec<(WeaponClass, String)> {
    WeaponClass::ALL
        .into_iter()
        .filter_map(|c| arc_class_display(card, arc, c).map(|s| (c, s)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dmg(s: &str, m: &str, l: &str, e: &str) -> ArcDamage {
        ArcDamage { s: s.into(), m: m.into(), l: l.into(), e: e.into() }
    }

    /// A Union-like spheroid DropShip: Nose STD + MSL, an empty Left arc, a light Aft.
    fn union() -> ArcCard {
        ArcCard {
            front: FiringArc {
                std: dmg("4", "3", "2", "0*"),
                msl: dmg("1", "1", "1", "1"),
                specials: vec!["PNT1".into()],
                ..Default::default()
            },
            rear: FiringArc { std: dmg("1", "0", "0", "0"), ..Default::default() },
            ..Default::default()
        }
    }

    #[test]
    fn arc_damage_parses_and_preserves_minimal() {
        let c = union();
        let front_std = arc_damage(&c, Arc::Nose, WeaponClass::Std);
        assert_eq!((front_std.s, front_std.m, front_std.l), (4.0, 3.0, Some(2.0)));
        assert_eq!(front_std.e, Some(0.5), "0* → minimal 0.5");
        assert!(is_zero(&arc_damage(&c, Arc::Nose, WeaponClass::Cap)), "absent class = all zero");
    }

    #[test]
    fn arc_lines_skips_empty_classes() {
        let c = union();
        let lines: Vec<WeaponClass> = arc_lines(&c, Arc::Nose).into_iter().map(|(w, _)| w).collect();
        assert_eq!(lines, vec![WeaponClass::Std, WeaponClass::Msl], "Nose carries STD + MSL only");
        assert!(arc_lines(&c, Arc::Left).is_empty(), "an empty arc has no lines");
    }

    #[test]
    fn random_weapon_class_buckets() {
        // 1D6 → three buckets (p.190); SCAP folds into the 3-4 Capital-non-missile bucket.
        assert_eq!(random_weapon_class(1), Some(WeaponClass::Std));
        assert_eq!(random_weapon_class(2), Some(WeaponClass::Std));
        assert_eq!(random_weapon_class(3), Some(WeaponClass::Cap));
        assert_eq!(random_weapon_class(4), Some(WeaponClass::Cap));
        assert_eq!(random_weapon_class(5), Some(WeaponClass::Msl));
        assert_eq!(random_weapon_class(6), Some(WeaponClass::Msl));
        assert_eq!(random_weapon_class(0), None);
        assert_eq!(random_weapon_class(7), None);
    }

    #[test]
    fn to_hit_mods_and_threshold() {
        // SBF/capital-scale table (p.191), reserved for the SBF phase.
        assert_eq!(WeaponClass::Cap.to_hit_mod(), 3);
        assert_eq!(WeaponClass::ScAp.to_hit_mod(), 2);
        assert_eq!(WeaponClass::Msl.to_hit_mod(), 0);
        // Standard-BF table (p.83, footnote 28): CAP +5 / SCAP +3 vs a small aerospace target,
        // +0 for missiles/standard and +0 vs any non-small target (large craft, ground).
        assert_eq!(WeaponClass::Cap.bf_vs_small_mod(true), 5);
        assert_eq!(WeaponClass::ScAp.bf_vs_small_mod(true), 3);
        assert_eq!(WeaponClass::Msl.bf_vs_small_mod(true), 0);
        assert_eq!(WeaponClass::Std.bf_vs_small_mod(true), 0);
        assert_eq!(WeaponClass::Cap.bf_vs_small_mod(false), 0, "waived vs a large / ground target");
        assert!(WeaponClass::Msl.is_capital() && !WeaponClass::Std.is_capital());
        assert!(threshold_triggered(5.0, 5));
        assert!(!threshold_triggered(4.0, 5));
        assert!(!threshold_triggered(0.5, 1), "minimal never meets a threshold");
        assert!(!threshold_triggered(10.0, 0), "no threshold → never triggers");
    }

    #[test]
    fn arc_display_preserves_printed_strings() {
        let c = union();
        assert_eq!(arc_class_display(&c, Arc::Nose, WeaponClass::Std).as_deref(), Some("4/3/2/0*"));
        assert_eq!(arc_class_display(&c, Arc::Nose, WeaponClass::Cap), None, "empty class → None");
        assert_eq!(
            arc_display_lines(&c, Arc::Nose),
            vec![
                (WeaponClass::Std, "4/3/2/0*".to_string()),
                (WeaponClass::Msl, "1/1/1/1".to_string()),
            ]
        );
        assert!(arc_display_lines(&c, Arc::Left).is_empty());
    }
}
