# Strategic BattleForce — Design & Reference

Strategic BattleForce (SBF) is the company-and-above scale of BattleTech from *Interstellar Operations: BattleForce* (IO:BF). Where Alpha Strike (AS) tracks one card per combat unit, SBF folds several AS elements into an **SBFUnit** and 1–4 SBFUnits into an **SBFFormation** — the piece a player actually moves and fires. Each aggregation halves or thirds and re-rounds the underlying AS numbers, so SBF is a strict *derivation on top of the AS data neurohelmet already bakes*. The ladder:

```
AS element  (a Mech + its AsStats)                 — the base data neurohelmet bakes
   └─▶ SBFUnit      (1–6 elements → one stat line)  — SBFUnitConverter port
          └─▶ SBFFormation (1–4 units → one stat line) — SBFFormationConverter port
                 └─▶ live combat tracking
```

neurohelmet already provides the whole bottom rung: `GameMode::{Classic,AlphaStrike,Override}`, an uncapped `Vec<TrackedMech>` roster in AlphaStrike sessions, per-element AS live tracking (`AsCrits`/`AsTarget`/armor/structure/heat), a deterministic-converter-plus-live-hit-maps pattern (`override_conv.rs`), and forcegen that emits the named formations SBF uses (`FORMATIONS = [(4,"Lance"),(5,"Star"),(6,"Level II"),(12,"Company")]`). SBF is a fourth `GameMode` that groups those elements into units and formations, recomputes the derived stat lines on demand (exactly as `ov_card()` recomputes the Override card from the spec), and persists only the live combat counters.

This document stands in for the MegaMek source: every formula, rounding rule, threshold and SUA list is transcribed inline with its source-file citation. MegaMek's interactive SBF engine (`server/sbf/…`) is an admitted stub; its complete combat logic lives in the auto-resolve (ACAR) engine. Both are cited; every gap and contradiction is flagged **NEEDS RULEBOOK** in the Known limitations section.

**One methodological invariant governs the whole document and resolves several apparent contradictions below.** The conversion layer (element → unit → formation) is a *bug-for-bug port of MegaMek*, validated by golden tests asserting neurohelmet's `convert_unit`/`convert_formation` equal MegaMek's `SBFUnitConverter`/`SBFFormationConverter` output. The combat layer is where neurohelmet *owns the rules* and applies the intended IO:BF corrections. **Where a conversion-side behavior looks wrong (turret SUAs, dead branches, double-division), neurohelmet reproduces MegaMek exactly and flags it — it does not fix it in conversion, because a fix would break the golden.** Where those two mandates conflict, the golden invariant wins for the conversion layer.

---

## Scope: single force by design

**neurohelmet is a record-sheet tracker, and real BattleForce is tracked single-force.** At the table each player fills out record sheets for *their own* formations — each Unit's armor circles, the DMG/TGT/MP crit boxes, morale status. There is no "both sides on one sheet" artifact; the printed IO:BF record sheets confirm this. neurohelmet therefore tracks **one force — yours — exactly like AlphaStrike/Override mode.**

**Two-sided tracking (OpFor roster, `side` fields, per-side initiative, turn interleave) is a deliberate NON-GOAL.** It appears in the body below *only* because the combat **rules** were transcribed from MegaMek's game / auto-resolve **engine** (`SBFGame`, `server/sbf/*`, `autoresolve/acar/*`), which plays the whole battle for both players and is therefore inherently two-sided. That two-sidedness is an artifact of the **simulator**, not of BattleForce as played — MegaMek's own *record-sheet* renderer (`SBFRecordSheet`), and the Mekbay record-sheet app, are single-force, exactly like neurohelmet. Building the two-sided model into neurohelmet would turn a record sheet into a solo/GM battle simulator — a different product, out of scope. The initiative and turn-interleave those engine files model are a **live table procedure the two players resolve together with dice**, not persistent state a record sheet stores; they stay at the table.

The only cross-side information a player ever needs is the **target's TMM/morale to roll to-hit** — hand-entered, exactly as read off the opponent's sheet across the table. That is a faithful digital analog of physical play, and it is all the "opponent" neurohelmet requires.

**The two-sided material in the body (§3.2 `side`/`sides`/`SbfSideState`, §4.4 initiative, §4.5 interleave) is retained only as a paper trail for that out-of-scope simulator idea; it is not implemented.** Single-force is a strict **subset** of everything specified here: conversion, live armor/crit/morale tracking, to-hit vs a hand-entered target, and damage/crits/crippling are all fully written up below. The deltas from the two-sided body to the shipped single-force design:

- **§3.2 state** — `SbfSideState` and `SbfState.sides` are dropped; `SbfFormationState.side` is dropped. `SbfState.formations` holds only your formations (mirrors the AS roster = only your mechs). Everything else is kept: `round`, `active_formation`/`active_unit`, per-formation `morale`/`jump_used_this_turn`/`is_done`, and all `SbfUnitState` counters. (`high_stress` is also dropped — morale is a manual rung, §4.3; and `MoraleStatus` has four rungs, no `Unsteady`.)
- **§3.5 grouping ops** — the `side: u8` argument is dropped from every signature (`sbf_new_formation(name, pool_range)`, etc.). No OpFor grouping.
- **§4.1 to-hit** — unchanged math. The target is **always hand-entered** via an `SbfTarget { target_tmm, target_jumped, target_morale, num_targets }` editor mirroring `AsTarget`/`OvShot`. `with_target` (tracked-OpFor lookup) is not built.
- **§4.2 damage / §4.3 morale / §4.6 crippling** — **kept in full.** All are single-force: you mark damage/crits on *your* units, your formations check morale when *they* take stress, your formations can be crippled. No opponent state needed.
- **§4.4 initiative + §4.5 turn interleave** — **not built (non-goal).** With no tracked OpFor there is nothing to interleave, and the interleave is a table procedure anyway. "Activation" is just a `round: u32` counter plus a per-formation `is_done` toggle over your own formations (advance the round when all are done). `roll_initiative` and the movement-ratio interleave are omitted.
- **§5.2 / §5.3 UI** — single-force panes: formations → units → stat line + `A:▓▓▓░░` armor track + `DMG/TGT/MP` crit counters + live morale; plus a target-entry line exactly like AS mode's `AsTarget` editor for computing to-hit. No two-side interleave view; no `n` (roll side init) keybinding.

The kept parts apply as written. When the two-sided body and this section conflict, **this section wins.**

---

## The BattleForce ladder

BattleForce is a ladder of ever-coarser scales (all in IO:BF), and this document covers one rung of it. The full ladder, with page-cited definitions:

| System | Playing piece | Tracked at | neurohelmet status |
|---|---|---|---|
| Total Warfare / Classic | 1 'Mech | full record sheet | `GameMode::Classic` |
| Alpha Strike | 1 element | AS card | `GameMode::AlphaStrike` |
| Standard BattleForce | lance (Unit) — moves as a stack | per element | `GameMode` (Standard BattleForce) |
| **Strategic BattleForce** | Formation (company) | per Unit (lance fused to one stat line) | `GameMode::StrategicBattleForce` (this document) |
| Abstract Combat System (ACS) | Formation / combat command | abstract areas | `GameMode` (ACS) |

Key rulebook anchors: IO:BF p.25 defines an **Element** ("equivalent to Unit, as used in Total Warfare… the smallest organizational grouping") and a **Unit** ("the smallest grouping of Elements — a Lance, Star, Level II and so on"); p.28 Movement Basics: *"A Unit's MP always equals the lowest MP of any of its surviving Elements. … All Elements in a Unit move at the same time and to the same hex"* — but combat stays per-element (*"Each surviving Element of each Unit may make one attack"*). p.163: in SBF *"the individual playing piece represents a Formation."* p.238: ACS *"is intended to replicate large invasions of worlds."* So Standard BF is the **lance-movement** game (stack moves together, elements fight and track individually); SBF fuses the lance into one stat line; regiment/battalion abstraction is ACS territory, not SBF.

---

## Reference sources

MegaMek paths are relative to the source tree `megamek/src/megamek/`.

**Conversion:**
- `common/strategicBattleSystems/SBFUnitConverter.java` — element → SBFUnit (the authoritative unit converter).
- `common/strategicBattleSystems/SBFFormationConverter.java` — force → SBFFormation pipeline.
- `common/strategicBattleSystems/BaseFormationConverter.java` — the formation-stat aggregation (`calcSbfFormationStats`, SUA aggregation, tactics, morale rating) and the `canConvert` structural-validity gate.
- `common/strategicBattleSystems/SBFUnit.java` — runtime SBFUnit model + in-game crit fields.
- `common/strategicBattleSystems/SBFFormation.java` — runtime formation model, `MoraleStatus`, artillery tables, the narrow formation `isAerospace()`.
- `common/strategicBattleSystems/SBFElementType.java` — the 10 SBF types + ground/aero classification (the broad `isAerospace()`).
- `common/strategicBattleSystems/SBFMovementMode.java` — movement modes, codes, ranks, `modeForElement`.
- `common/alphaStrike/{ASDamage,ASDamageVector,AlphaStrikeElement,ASSpecialAbilityCollection,ASSpecialAbilityCollector}.java` — `0*`→0.5, `reducedBy`, SUA merge/set semantics, and the crucial top-level-only `hasSUA` (see Data-fidelity trap #1).

**Combat, interactive path (structure, but stubbed):**
- `common/strategicBattleSystems/SBFToHitData.java` — to-hit compilation.
- `common/actions/sbf/SBFStandardUnitAttack.java` — attack action + `isDataValid`.
- `server/sbf/SBFStandardUnitAttackHandler.java` — damage/crit resolver stub.
- `server/sbf/SBFAttackProcessor.java`, `SBFGameManager.java`, `SBFPhasePreparationManager.java`, `SBFPhaseEndManager.java`, `SBFInitiativeHelper.java`, `SBFMovementProcessor.java` — turn/init/movement plumbing.
- `server/sbf/SBFDetectionHelper.java`, `SBFDetectionModifiers.java`; `common/strategicBattleSystems/SBFVisibilityStatus.java`, `SBFVisibilityHelper.java` — detection/visibility.

**Combat, ACAR path (the complete reference for crits/destruction/morale):**
- `common/autoresolve/acar/handler/StandardUnitAttackHandler.java` — full damage + crit table.
- `common/autoresolve/acar/action/AttackToHitData.java` — the divergent to-hit.
- `common/autoresolve/acar/phase/EndPhase.java`, `VictoryPhase.java` — destruction rollup, morale, withdrawal.
- `common/autoresolve/component/Formation.java` — `isCrippled()`, `getCurrentMovement()`.
- `common/autoresolve/acar/action/{RecoveringNerveActionToHitData,ManeuverToHitData}.java`, `.../handler/{MoraleCheckActionHandler,RecoveringNerveActionHandler}.java` — morale mechanics.
- `common/{InitiativeRoll,TurnOrdered}.java` — 2d6 team initiative, tie-break.

**neurohelmet (to modify / mirror):**
- `crates/core/src/domain.rs` — `GameMode`, `AsStats`, `Mech`, `Mech::{is_aerospace,is_vehicle,display_name}`.
- `crates/core/src/session.rs` — `TrackedMech`, `Session`, `AsCrits`/`AsTarget`/`CtTarget`/`OvShot`, the AS live-tracking methods, `mech_cap`/`add_mech`/`point_cost`/`force_total`.
- `crates/core/src/engine/override_conv.rs` — the converter-plus-hit-maps pattern to mirror; golden test `golden_cards_end_to_end`.
- `crates/core/src/engine/alpha_strike.rs` — `movement_hexes`, `as_to_hit_full`.
- `crates/core/src/engine/mod.rs` — module registration.
- `crates/app/src/tui/{app.rs,view.rs,forcegen.rs}` — `Screen`, mode dispatch, `PendingAction::NewSession(GameMode)`, `FORMATIONS`.

---

## Data fidelity and gaps

`AsStats` is the entire converter input. Every string field is copied **byte-for-byte from Mekbay** (`crates/bake/src/join.rs` `parse_as_stats`); neurohelmet performs **zero** numeric parsing at bake time, so all typing below is the parser's job.

```
pub struct AsStats {
    pub pv: u16,          // skill-4 BASE PV (confirmed: no skill param anywhere). Range 1..99.
    pub size: u8,
    pub tp: String,       // AS unit type code: BM CI CV BA AF SV IM CF  + neurohelmet-local "BD"
    pub movement: String, // verbatim, e.g. `6"`  `10"/8"j`  `7a`  `0"t`
    pub tmm: u8, pub armor: u8, pub structure: u8,
    pub dmg_s/m/l/e: String, // each independently one of: "0" | "0*" | <int>
    pub overheat: u8,        // OV
    pub threshold: u8,       // TH (aerospace); 0 for ground. `usesTh` is NOT baked → infer TH>0.
    pub specials: Vec<String>, // each element one SUA token, already split
}
```
The per-element **skill** is *not* in `AsStats`; it lives on `TrackedMech.gunnery` (AS PV uses `skill_adjusted_pv(pv, gunnery)`). So an AS element's SBF skill = its `TrackedMech.gunnery`.

### Exact parser rules (measured against the full baked catalog)

**Damage cell** (`dmg_s/m/l/e`, and any bracket inside a vector): exactly three forms exist catalog-wide.
```rust
fn as_damage(s: &str) -> f32 { match s.trim() { "0*" => 0.5, "-" => 0.0, x => x.parse().unwrap_or(0.0) } }
```
`0*` = minimal = **0.5** (this is `ASDamage.asDoubleValue()`, `ASDamage.java`). `-` never appears in the four main fields, only inside vectors (= no bracket → `None`).

**Movement** (`/`-joined tokens):
- ground token = `<int>"<mode?>` (has the literal `"`); aero token = `<int>a` (no `"`).
- `primary` = int of first token. `jump` = int of the token whose suffix is exactly `j` (a lone `6"j` is jump-only → its value is both primary and jump). `mode` = first token's suffix (`""`→Walk, `a`→AeroThrust, `j t w h m v s g n r f qt qw` per §2.2). `primary==0 && mode==Walk` → Immobile.
- LAM/BIM alt modes are **not** in `movement`; they live in the `LAM(…)`/`BIM(…)` specials — the `movement` field of a LAM holds only its ground 'Mech move.

**Specials** (859 distinct tokens catalog-wide). The parser's `parse_special` must special-case, because the naive "leading letters = code, trailing digits = value" split breaks on: `C3*` (digit inside code), `ART*-<n>` (hyphen + possibly-embedded digit like `ARTCM5-1`), `I-TSM`/`TSEMP-O1` (hyphen), `CF:N` (colon, neurohelmet-local), and `IT#`/`CT#` (**decimal** values, e.g. `IT1.95`, `CT0.009`). Bracket tokens inside a vector are `int | 0* | -`; vectors are 2, 3, or 4 brackets long.

### SUAs the converter needs vs. what the data can supply

Every SUA the SBFUnit/Formation converters read **is present** in the baked specials: IF, FLK, AC, CAR, IT, CT, ARTx (all 10 subtypes), MHQ, RCN, STL, OMNI, ENE, CASE/CASEII, CR, RAMS, AMS, OVL (overheat magnitude is the separate `overheat:u8`). The converter must nonetheless handle these fidelity traps:

1. **Turret-embedded SUAs — parse them, but the converter must DELIBERATELY IGNORE them (MegaMek parity).** IF/TAG/AMS/ART*/INARC/SNARC/REL/LTAG/TSEMP/RAMS/HT/AC/SRM/FLK/LRM/TOR/MTAS/C3M2/MHQ can appear **only inside `TUR(…)`** with no top-level duplicate (e.g. `TUR(2/2/1,IF1)` has no separate `IF1`). It is tempting to union these into the element's flat SUA set — **but MegaMek's converter never sees turret abilities, and matching MegaMek is the golden invariant.** `AlphaStrikeElement.hasSUA` (`AlphaStrikeElement.java`) delegates to `ASSpecialAbilityCollection.hasSUA`, which is `containsKey` on the **top-level map only** (`ASSpecialAbilityCollection.java`); `SBFUnitConverter` reads element SUAs solely via `e.hasSUA(SUA)` / `e.getSUA(SUA)` / `e.getStandardDamage()` and **never** calls `getTUR()`/`getFrontArc()`. So a turret-only IF/AC/ART/FLK etc. is invisible to the converter. **Therefore the parser reads `TUR(…)` interior SUAs into a SEPARATE `turret_suas` map that `has_sua`/`sua_num`/`sua_dmg` do NOT read, and that `convert_unit` never touches.** Turret contents are not unioned into the flat `suas` set. This is why the named `Demolisher` golden fixture (an all-turret vehicle) passes: MegaMek derives its SBFUnit from front/standard damage only, ignoring the turret arsenal, and so must neurohelmet. The rulebook-correct behavior (turret weapons *should* contribute IF/FLK/ART/etc.) is a deliberate divergence; see Known limitations (turret abilities excluded from conversion).
2. **Front-arc only.** The four `dmg_*` fields are the front/standard vector (`element.getStandardDamage()`). Rear/turret/torso damage exists only as `REAR#/#/#`, `TUR(…)`, `TOR#/#/#`. The SBF damage sums use standard (front) damage; FLK aggregation uses the **top-level** `FLK`/`AC` *SUA vectors* only (turret-nested `FLK`/`AC` live in `turret_suas` and are ignored, per trap #1 / MegaMek parity).
3. **`0*`→0.5 / `-`→None live inside vectors too** — don't reuse an int parser for brackets.
4. **Decimal SUAs** — `IT`/`CT` parse as `f32`.

**Gaps the data cannot supply (workarounds):**
- **`isAerospaceSV()`** — needed to route `SV`→`AS` vs `V`. neurohelmet can't distinguish. Workaround: route all `SV` → `V` (the common case); flag NEEDS RULEBOOK.
- **`PM`/`MS`/large-craft (`DS/DA/JS/WS/SS`) AS types** — not baked (catalog is BM/CI/CV/BA/AF/SV/IM/CF). Map the `default` branch → `LA` if ever seen; the neurohelmet-local `BD` (gun emplacement) → `V`. Flag.
- **Quirks** (`RCN` name/quirk test reads `QUIRK_POS_IMPROVED_SENSORS`) — not baked. Workaround: implement only the name-substring and numeric legs of `isConsideredRcn`; drop the quirk clause. Flag.
- **`CF:N` / `IMMOBILE`** (neurohelmet-local, from `data/extra_units.json`) — no standard SBF SUA; **whitelist-skip** them in the parser rather than choke.

---

## 1. The typed AS element and SUA parser

The parser lives in `crates/core/src/engine/as_element.rs` (registered in `engine/mod.rs` as `pub mod as_element;`). It is a pure function of `AsStats` plus a skill byte — standalone and independently unit-testable without the converter.

### Types

```rust
/// A bracketed damage vector (2–4 bands). `-`→None, `0*`→0.5.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DamageVector { pub s: f32, pub m: f32, pub l: Option<f32>, pub e: Option<f32> }

/// SBF element type (SBFElementType.java). Ordering is not meaningful; it is a plain enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SbfElementType { Unknown, Bm, As, Mx, Pm, V, Ba, Ci, Ms, La }

impl SbfElementType {
    pub fn is_ground(self) -> bool {
        matches!(self, Self::Bm|Self::Mx|Self::Pm|Self::V|Self::Ba|Self::Ci|Self::Ms)
    }
    /// BROAD aerospace test (SBFElementType.java) — Unknown, As, La count as aerospace.
    /// This mirrors SBFUnit.isAerospace and is the predicate the UNIT-movement converter uses
    /// (§2.3 step 3). It is DISTINCT from the narrow formation test in §2.4 — do not
    /// reuse this one for formation movement or you will pick min-thrust for Unknown formations
    /// where MegaMek uses the mean.
    pub fn is_aerospace(self) -> bool { !self.is_ground() }
}
```

`SbfRange` (the S/M/L/E firing bracket) and the `DamageVector::band(range)` accessor are combat concepts defined in §4.1–4.2; since `DamageVector` is defined here in the same crate, the inherent `band` impl lives in `sbf.rs`. They are cross-referenced here only so the reader knows a band selector exists.

```rust
/// One resolved SUA value. TUR contents are parsed into a SEPARATE turret map (see below),
/// NEVER into this enum's home map, to preserve MegaMek converter parity (Data-fidelity trap #1).
#[derive(Clone, Debug, PartialEq)]
pub enum SuaVal {
    Flag,                 // presence only: CASE, ENE, STL, OMNI, RCN, AMS, CR, RAMS, OVL, SOA, LG, ...
    Num(f32),             // IF1, CAR4, IT1.95, CT0.009, MHQ0, BOMB2, FUEL20, OMNI count, ...
    Art(u8),              // ARTAIS-2 → count 2
    Dmg(DamageVector),    // AC, FLK, HT, LRM, SRM(2-band), TOR, IATM, REAR
    Lam { wige: u32, thrust: u32 },
    Bim(u32),
}

/// Everything the SBF converters read off one AS element.
#[derive(Clone, Debug)]
pub struct AsElement {
    pub name: String,                 // for isConsideredRcn name tests
    pub as_type: String,              // raw AS tp ("BM","CV",...); the string mode_for_element matches
                                      //   its type qualifiers against (see §2.2), and the source for
                                      //   the isAerospaceSV routing note.
    pub sbf_type: SbfElementType,     // getUnitType() result (mapping below)
    pub size: u8,
    pub primary_move: u32,            // inches, first token
    pub primary_mode: String,         // first token's suffix ("" , "j","t","w","h","v","s","g","n","r","f","m","qt","qw","a")
    pub jump_move: u32,
    pub skill: u8,                    // TrackedMech.gunnery; default 4
    pub full_armor: u16,              // AsStats.armor
    pub full_structure: u16,          // AsStats.structure
    pub std_damage: DamageVector,     // front S/M/L/E from dmg_s..dmg_e
    pub overheat: u8,                 // OV
    pub threshold: u8,                // TH
    pub suas: BTreeMap<String, SuaVal>,        // TOP-LEVEL only; the ONLY map the converter reads
    pub turret_suas: BTreeMap<String, SuaVal>, // TUR(...) interior; parsed for fidelity/UI, IGNORED by convert_unit
    pub base_pv: u16,                 // AsStats.pv (skill-4 base)
}
```

### `getUnitType` mapping (`SBFElementType.java`) — build `sbf_type` from `tp`

| `tp` | `sbf_type` |
|---|---|
| `IM`, `BM` | `Bm` |
| `PM` | `Pm` |
| `MS` | `Ms` |
| `BA` | `Ba` |
| `CI` | `Ci` |
| `AF`, `CF`, `SC` | `As` |
| `CV` | `V` |
| `SV` | `As` if `isAerospaceSV()` else `V` — **neurohelmet: always `V`** (gap; flag) |
| `BD` (neurohelmet-local) | `V` |
| default | `La` |

### SUA query helpers (mirror `ASSpecialAbilityCollector`)

```rust
impl AsElement {
    /// Top-level presence only, mirroring AlphaStrikeElement.hasSUA → ASSpecialAbilityCollection
    /// containsKey (top-level map). Turret SUAs are deliberately NOT visible here (trap #1).
    pub fn has_sua(&self, code: &str) -> bool { self.suas.contains_key(code) }
    pub fn has_any_sua(&self, codes: &[&str]) -> bool { codes.iter().any(|c| self.has_sua(c)) }

    /// getSUA-as-number. Mirrors MegaMek's numeric getters (getCAR/getIT/getMHQ/...): the stored
    /// numeric value, or 0 when the code is absent. `Num(v)→v`, `Art(c)→c`, `Dmg(d)→d.s`.
    ///
    /// The `Flag→1.0` case is NOT a general MegaMek behavior — MegaMek never reads a presence-only
    /// flag through a numeric getter in the ported converter paths (it would be a null value). It is
    /// provided here ONLY so the §2.3-step-9 aggregation merge helper can mirror MegaMek's merge rule
    /// `null value → +1`. DO NOT rely on `Flag→1` in any general numeric read (e.g. the dead CAR<=IT
    /// transport branch treats missing CAR/IT as 0, which is exactly what this returns for absent codes).
    pub fn sua_num(&self, code: &str) -> f32 { /* Num(v)=>v, Art(c)=>c as f32, Dmg(d)=>d.s, Flag=>1.0, missing=>0.0 */ }

    pub fn sua_dmg(&self, code: &str) -> Option<DamageVector> { /* Some(d) iff top-level Dmg */ }
    pub fn get_ov(&self) -> f32 { self.overheat as f32 }
    /// fuelRating (SBFUnitConverter.java): fuel = FUEL value; if fighter && fuel==0 → 4.
    pub fn fuel_rating(&self) -> f32 { /* FUEL num; if as_type in {AF,CF} && ==0 { 4 } */ }
}
```

### Parser (the core deliverable)

```rust
pub fn as_element(stats: &AsStats, name: &str, skill: u8) -> AsElement;
fn as_damage(s: &str) -> f32;                                  // "0*"→0.5, "-"→0.0
fn parse_bracket(s: &str) -> Option<f32>;                      // "-"→None, "0*"→Some(0.5), int→Some
fn parse_damage_vector(s: &str) -> DamageVector;               // split '/', 2–4 bands
fn parse_movement(mv: &str) -> (u32 /*primary*/, u32 /*jump*/, String /*mode*/);
/// Routes each token into either the flat top-level map `out` or, for TUR interior, the `turret` map.
fn parse_special(tok: &str, out: &mut BTreeMap<String, SuaVal>, turret: &mut BTreeMap<String, SuaVal>);
```
`parse_special` rules: `TUR(` → parse a leading bare vector as the front vector (ignored for SBF), then comma-split the remainder and recurse each interior token **into `turret`, NOT `out`** (this is the deliberate non-union that keeps the converter MegaMek-faithful — trap #1); `LAM(` / `BIM(` → `Lam{wige,thrust}` / `Bim`; `ART`→`Art{subtype,count}` splitting on the **last** `-`; token containing `:` or equal to `IMMOBILE` → **skip** (neurohelmet-local); a known damage-vector code (`AC FLK HT IATM LRM SRM TOR REAR`) containing `/` → `Dmg`; a C3-family / hyphen token → match whole token as `Flag`; a trailing numeric run (incl. decimal) → `Num`; else `Flag`. Non-TUR tokens always go to `out`.

### Rounding helper (used by all layers — defined here or in `sbf.rs`)

```rust
/// Java `Math.round(double)` = floor(x + 0.5): round half UP toward +∞.
/// NOTE this differs from Rust f64::round (half AWAY from zero) for negative halves:
/// jround(2.5)==3, jround(-2.5)==-2.
#[inline] pub fn jround(x: f64) -> i64 { (x + 0.5).floor() as i64 }
```

---

## 2. SBF conversion: unit and formation

The conversion layer lives in `crates/core/src/engine/sbf.rs` (the combat submodule is added to the same file; see §4), registered as `pub mod sbf;`. It depends on §1 (`as_element::{AsElement, SbfElementType, DamageVector, SuaVal, jround}`).

This layer ports `SBFUnitConverter.java` + `BaseFormationConverter.java` **faithfully, including MegaMek's documented bugs**, because the golden tests compare against MegaMek converter output. Every bug is annotated; it is not "fixed" here (fixes, if any, belong to the combat layer where neurohelmet owns the rules). In particular, `convert_unit` reads **only** `AsElement.suas` (top-level) — never `AsElement.turret_suas` — exactly as `SBFUnitConverter` reads only the top-level SUA map.

### 2.1 Output types (derived, immutable — recomputed on demand, never persisted)

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct SbfUnit {
    pub name: String,
    pub sbf_type: SbfElementType,
    pub size: u8,
    pub movement: i64,          // "MV"
    pub move_mode: SbfMoveMode,
    pub jump_move: i64,
    pub trsp_movement: i64,
    pub trsp_mode: SbfMoveMode,
    pub tmm: i64,               // no clamp (SBFUnitConverter has none)
    pub armor: i64,             // full; live current = armor - armor_hits
    pub damage: DamageVector,   // S/M/L(/E) integral
    pub skill: i64,             // clamped 0..=7
    pub point_value: i64,       // floor 1
    pub suas: BTreeMap<String, SuaVal>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SbfFormation {
    pub name: String,
    pub sbf_type: SbfElementType,
    pub size: i64, pub tmm: i64, pub movement: i64, pub move_mode: SbfMoveMode,
    pub jump_move: i64, pub trsp_movement: i64, pub trsp_mode: SbfMoveMode,
    pub tactics: i64, pub morale_rating: i64, pub skill: i64, pub point_value: i64,
    pub suas: BTreeMap<String, SuaVal>,
    pub units: Vec<SbfUnit>,
}

impl SbfFormation {
    /// NARROW formation aerospace test (SBFFormation.java): `As` or `La` ONLY.
    /// This is DISTINCT from SbfElementType::is_aerospace() (which also returns true for Unknown).
    /// It governs (a) formation Movement = min-vs-mean (§2.4) and (b) any E-band/aero-only formation
    /// gating. An implementer must NOT reuse the broad element/unit predicate here, or a degenerate
    /// `Unknown`-typed formation would take min-movement where MegaMek takes the mean → silent golden
    /// mismatch.
    pub fn is_aerospace(&self) -> bool { matches!(self.sbf_type, SbfElementType::As | SbfElementType::La) }
}

/// Firing range bracket, chosen by hand (no board). Also used by DamageVector::band (§4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SbfRange { Short, Medium, Long, Extreme }
```
`SbfMoveMode` is a Rust enum mirroring `SBFMovementMode` with a `.code()->&str` and `.rank()->i32`; the full table is below.

### 2.2 `SbfMoveMode` — code + rank (`SBFMovementMode.java`)

Lower rank = **more restrictive** (adopted preferentially; ties → first-seen). Use the numeric rank, not declaration order.

| variant | code | rank |
|---|---|---|
| Naval | `n` | 0 |
| Submarine | `s` | 0 |
| Rail | `r` | 10 |
| StationKeeping | `k` | 11 |
| Wheeled | `w` | 20 |
| Spheroid | `p` | 21 |
| Hover | `h` | 30 |
| Warship | `aw` | 31 |
| Tracked | `t` | 40 |
| Airship | `i` | 41 |
| BimAerospaceWalk | `` | 50 |
| BaWalk | `` | 50 |
| BaUmu | `s` | 51 |
| InfantryFoot | `f` | 52 |
| CiJump | `` | 53 |
| Aerodyne | `a` | 54 |
| MekWalk | `` | 60 |
| BaJump | `` | 61 |
| MekUmu | `s` | 62 |
| QuadTracked | `qt` | 63 |
| QuadWheeled | `qw` | 64 |
| LamAerospaceWalk | `` | 65 |
| MekJump | `` | 70 |
| Vtol | `v` | 80 |
| Wige | `g` | 81 |
| Unknown | `Unknown` | i32::MAX |

**`mode_for_element(unit_is_aero: bool, el: &AsElement) -> SbfMoveMode`** (`SBFMovementMode.java`). **The type qualifiers below match on `el.as_type` (the raw AS `tp` string, e.g. `"BM"`,`"CV"`,`"SV"`,`"BA"`,`"WS"`), not the collapsed `sbf_type`** — because MegaMek's `modeForElement` switches on the AS unit type. For neurohelmet's baked catalog the collapsed types route identically for every reachable case (`SV`→`Submarine` == its collapse `V`→`Submarine`), with one theoretical exception: `WS→Warship`. **`WS` is not in the baked catalog, so the `Warship` branch is unreachable in neurohelmet** — implement it for completeness but expect it never to fire.
- If `unit_is_aero && el.sbf_type.is_ground()`: `BIM`→`BimAerospaceWalk`; else `LAM`→`LamAerospaceWalk`; else `SOA`→`StationKeeping`; else `Unknown`.
- Otherwise switch on `el.primary_mode` string, with `el.as_type` qualifiers:
  - `""` & type BM/PM/IM → `MekWalk`; `""` & WS → `Warship` (unreachable in neurohelmet); `""` & BA → `BaWalk`.
  - `w`/`w(b)`/`w(m)`/`m` → `Wheeled`; `v`→`Vtol`; `f`→`InfantryFoot`; `i`→`Airship`; `r`→`Rail`; `h`→`Hover`; `g`→`Wige`; `p`→`Spheroid`; `a`→`Aerodyne`; `qt`→`QuadTracked`; `qw`→`QuadWheeled`; `k`→`StationKeeping`; `n`→`Naval`; `t`→`Tracked`.
  - `s` & CV/SV→`Submarine`, `s` & BM/PM→`MekUmu`, `s` & BA→`BaUmu`.
  - `j` & BM/PM→`MekJump`, `j` & BA→`BaJump`, `j` & CI→`CiJump`.
  - else `Unknown`.

`most_restrictive(elems)` = start `Unknown`, adopt any mode with strictly smaller rank.

### 2.3 SBFUnit conversion — `pub fn convert_unit(name:&str, elems:&[AsElement]) -> SbfUnit`

**Order of operations is load-bearing** (`SBFUnitConverter.java`): type, size, move (sets `rounded_average_move`), mode, jump, transport, TMM, armor, **SUAs (populates unit SUAs)**, damage, skill, PV. Because TMM (step 7) and transport (step 6) run before SUAs (step 9), two branches read still-empty unit SUAs → **dead as written**; reproduce that behavior (see notes). All `round()` below is `jround`; `(int)` casts truncate toward zero; integer `/2` is floor.

**(1) Type:**
```
majority = jround(2.0/3.0 * n)        // n = elems.len(); 1→1 2→1 3→2 4→3 5→3 6→4
highest_type = first (list order) type with max frequency; ties → element order
type = if highest_count < majority { Mx } else { highest_type }   // fallback Unknown
```

**(2) Size:** `jround(mean(el.size))`; empty→0.

**(3) Movement** — sets `rounded_average_move` (init 0). **The aerospace test here is the BROAD one** (`unit_type.is_aerospace()`, matching `SBFUnit.isAerospace` used in `calcUnitMove`), so an `Unknown`-typed unit also takes the min-thrust path:
- Aerospace unit (`unit_type.is_aerospace()`): `movement = min over elems of effectiveThrust(e)`, where `effectiveThrust = if e.has_sua("SOA") {0} else {e.primary_move}`. **`rounded_average_move` stays 0** → TMM becomes `getTmmFromMove(0) = -4`. Per the Strategic Aerospace layer, air-to-air applies *no* target movement modifier and ground-to-air replaces TMM with a flat +2 airborne row (p.179/p.181), so the −4 never reaches a real to-hit; the converter stays faithful and the UI hides TMM on aero formations.
- Ground unit: `avg = mean(el.primary_move)`; `rounded_average_move = jround(avg/2.0)`.
  - If any element is infantry (type CI or BA): `min_inf = (min primary_move over infantry) / 2` **(integer floor)**; `movement = min(rounded_average_move, min_inf)`. Note the divergent rounding.
  - Else `movement = rounded_average_move`.
- **TMM keys off `rounded_average_move`, not `movement`** (important for infantry units where `movement < rounded_average_move`).

**(4) Movement mode:** `most_restrictive(elems)` via `mode_for_element` (pass `unit_is_aero = unit_type.is_aerospace()`).

**(5) Jump:** `jround(mean(el.jump_move)/4.0)`.

**(6) Transport move + mode:** `nonTransportable` = elems **not** having any of `MEC/XMEC/CAR`. Branch cascade:
1. all elems transport-capable → `trsp_movement=movement`, `trsp_mode=move_mode`.
2. else no elem has MEC/XMEC/CAR → same.
3. else `unit.CAR <= unit.IT` → **both are unit SUAs still 0 at this step, so `0<=0` is always true when reached** → `trsp_movement = jround(mean(primary_move over nonTransportable)/2.0)`, `trsp_mode = most_restrictive(nonTransportable)`.
4. else → **TBD stub / unreachable** → fall back to `movement`/`move_mode`. **NEEDS RULEBOOK.**

**(7) TMM:** `tmm = getTmmFromMove(rounded_average_move)` then deltas.
`getTmmFromMove(m)`: `m≥18→5, ≥10→4, ≥7→3, ≥5→2, ≥3→1, ≥1→0, else −4`.
Deltas, in order:
- `+1` if `unit_type ∈ {Ba,Pm}` OR (`unit_type==V` && move_mode.code ∈ {`v`,`g`}).
- `−1` if any element `has_sua("LG")`.
- `−2` if any element `has_any_sua(&["SLG","VLG"])`. *(−2, not −1: the code subtracts 2.)*
- `+2` if `unit.has_any_sua(&["STL","MAS"])` — **but unit SUAs aren't set yet at this step → never fires. Reproduce as dead** (flag).
No clamp.

**(8) Armor:** per element
```
v = el.full_armor + el.full_structure
delta = 0.0
if el.full_structure >= 3 || el.has_any_sua(&["AMS","CASE"]) { delta = 0.5 }
if el.has_any_sua(&["ENE","CASEII","CR","RAMS"]) { delta = 1.0 }   // OVERWRITES 0.5; max 1.0, never 1.5
v += delta;  sum += v
armor = jround(sum / 3.0)
```
`current_armor = armor` at init (`SBFUnit.java` — `setArmor` sets both).

**(9) Unit SUA aggregation** — `calc_unit_special_abilities`, executed in this exact order. `sua_count(code)` = #elements with it (top-level `suas` only). This populates `unit.suas`.

- **(a) IfAny (≥1)** → set flag: `WAT PRB AECM BHJ2 BHJ3 BH BT ECM HPG LPRB LECM TAG`.
- **(b) IfHalf (`max(1, n/2)`, floor)** → flag: `AMS ARM ARS BAR BFC CR ENG RBT SRCH SHLD`.
- **(c) IfAll (`n`)** → flag: `AMP AM BHJ XMEC MCS UCS MEC PAR SAW TRN`.
- **(d) sum (plain, no divide)** → `Num`: `CAR CK CT IT CRW DCC MDS MASH RSD VTM VTH VTS AT DT MT PT ST SCR`. Merge rule: `null→+1, int→+int, double→+double, ASDamageVector(1 band)→+S`. (This is the sole place `sua_num`'s `Flag→1` mapping is meant to apply — the `null→+1` merge.)
- **(e) sumDivideBy3** (`oneThird = jround(sum/3)`, keep only if `>0`) → `ATAC BOMB PNT IF`. For `IF` store `Num(oneThird)` (as ASDamage); else `replace` with `oneThird`.
- **(f) sumArtillery** (`count = Σ (int)value`; `artSum = count * artDamage(code)`; `value = jround(artSum/3)`; merge if `>0`) → `ARTLT ARTS ARTT ARTBA ARTCM5 ARTCM7 ARTCM9 ARTCM12`. `artDamage` table §2.5.
- **(g) MHQ:** if any MHQ, `total = Σ((int)MHQ − 1)` (**−1 per element**); `oneThird = jround(total/3.0)`; merge if `>0`.
- **(h) RCN:** `rcnCount = #elems where has_sua("RCN") || is_considered_rcn(e)`; if `≥2` set flag RCN. `is_considered_rcn` (any of): `type==BM && size≤2 && primary_move≥14`; `type∈{BM,PM} && jump_move≥12`; `is_ground && size≤2 && primary_move≥18`; name contains "Scout"/"Recon"/"Sensor"; *(quirk `IMPROVED_SENSORS` — **dropped, not baked; flag**)*.
- **(i) STL:** if **every** element `has_any_sua(&["STL","MAS","LMAS"])` → set flags **STL, MAS, LMAS** (all three). *(Mixing case is a class `//TODO` → partial NEEDS RULEBOOK.)*
- **(j) OMNI:** `omni = #elems with OMNI`; if `>0` merge `SBF_OMNI = Num(omni)`.
- **(k) FLK:**
  ```
  flkM = jround((Σ_FLK FLK.m + Σ_AC AC.m)/3);  flkL = jround((Σ_FLK FLK.l + Σ_AC AC.l)/3)
  if flkM+flkL > 0 { set FLK = Dmg{ s:0, m:flkM, l:Some(flkL) } }   // S=0, no min
  ```
  (Sums over **top-level** FLK/AC only; turret-nested copies in `turret_suas` are ignored — trap #1.)
- **(l) C3M/C3BSM→AC3:** if `(count(C3M)≥1 || count(C3BSM)≥1)` && `count(C3M)+count(C3S)+count(C3BSS) >= n/2` (floor) → set AC3.
- **(m) C3I→AC3:** if `count(C3I)>0 && count(C3I) >= n/2` (floor) → set AC3.
- **(n) FUEL:** `fuel = min over aerospace elements of fuel_rating(e)`; if present merge FUEL.
- **(o) ATAC /3 again:** if unit has ATAC, `replace ATAC = jround(ATAC/3.0)`. **Double-division bug (net ≈ sum/9); reproduce for parity, flag.**
- **(p) finalize:** if PRB → remove LPRB; if AECM → remove LECM & ECM; if ECM → remove LECM; if CT → merge its value into IT then remove CT.

**(10) Damage → `DamageVector`.** Artillery pre-sums: `artTC = count(ARTTC)*1`, `artLTC = count(ARTLTC)*3`, `artSC = count(ARTSC)*2`.
```
// S band:
dmgS = Σ el.std_damage.s                              // 0* already 0.5
ov = (Σ over ALL elems of el.get_ov()) / 2.0          // ALL elements
if ov>0 { dmgS += ov }
if unit_type ∈ {Ba,Ci} && unit.has_sua("AM") { dmgS += 1.0 }
if artTC+artLTC+artSC > 0 { dmgS += (artTC+artLTC+artSC) as f32 }
S = jround(dmgS/3.0)

// M band:
dmgM = Σ el.std_damage.m
ovM = (Σ over elems WHERE m>=1 of get_ov())/2.0 ; if ovM>0 { dmgM += ovM }
if artTC+artLTC+artSC>0 { dmgM += ... }
M = jround(dmgM/3.0)      // no AM bonus

// L band:
dmgL = Σ el.std_damage.l.unwrap_or(0)
ovL = (Σ over elems WHERE has_sua("OVL") && l>=1 of get_ov())/2.0 ; if ovL>0 { dmgL += ovL }
if artTC+artLTC>0 { dmgL += (artTC+artLTC) as f32 }   // artSC EXCLUDED at L
L = jround(dmgL/3.0)

// E band ONLY when unit_type == As:
if type==As { dmgE = Σ el.std_damage.e.unwrap_or(0); if artTC+artLTC>0 {dmgE += ...}; E = jround(dmgE/3.0) }
```
Non-`As` → `DamageVector{s:S,m:M,l:Some(L),e:None}`; `As` → include `e:Some(E)`.

**(11) Skill:** `skill = jround(mean(el.skill))` (default 4); `if has DN {−1}`; `if has_any(&["BFC","DRO","RBT"]) {+1}`; clamp `0..=7`. This is Step 1G (IO:BF p.259): a Unit with BFC, DRO or RBT has its Skill adjusted up by 1 (one point total, not cumulative). The p.172 attacker rows "Has the BFC special +1" / "Is a Drone +1" apply *in addition to* this Step-1G skill bake — conversion sets the stat, the attack table modifies the shot (§4.1); both are implemented. (In practice only BFC/RBT can move skill via this path; DN/DRO are never produced by the baked data.)

**(12) PV:**
```
sum = (Σ el.base_pv) / 3.0 ; intermediate = jround(sum) ; result = intermediate as f64
if skill > 4 { result = (1.0 - (skill-4) as f64 * 0.1) * intermediate }
else if skill < 4 { result = (1.0 + (4-skill) as f64 * 0.2) * intermediate;
                    result = result.max((intermediate + (4-skill)) as f64) }  // floor
point_value = max(1, jround(result))
```

### 2.4 Formation conversion — `pub fn convert_formation(name:&str, units:&[SbfUnit]) -> SbfFormation`

(`BaseFormationConverter.calcSbfFormationStats`. `k = units.len()` = 1..4. Order: skill precedes morale; movement/skill precede tactics. All averaged stats `jround(mean(...))`.)

- **Type:** `majority = jround(2.0/3.0*k)` (1→1 2→1 3→2 4→3); `highest_type` = first type in unit order at max frequency; `type = if highest_count<majority {Mx} else {highest_type}`.
- **Size** = `jround(mean(u.size))`.
- **Movement** = `jround(mean(u.movement))`, **except** if the formation is aerospace → `jround(min(u.movement))`. **The predicate here is the NARROW `SbfFormation::is_aerospace()` = `matches!(type, As|La)`** (`SBFFormation.java`), computed on the formation's derived type — **NOT** `SbfElementType::is_aerospace()` (which is broad and would also fire for `Unknown`/`Mx` degenerate formations, choosing min where MegaMek uses the mean). Concretely: evaluate `matches!(formation_type, As|La)` here.
- **Movement/Transport modes** = most-restrictive over unit modes.
- **TrspMovement** = `jround(mean(u.trsp_movement))`; **Jump** = `jround(mean(u.jump_move))`; **TMM** = `jround(mean(u.tmm))`; **Skill** = `jround(mean(u.skill))`.
- **Morale rating** = `3 + skill`. *(Runtime `MoraleStatus` is separate — §3.)*
- **Point Value** = **SUM** of `u.point_value` — not averaged.
- **SUAs:** four policies over `spaCount = #units with it`, all vs `k`:
  - **IfAny (≥1)** flag: `DN XMEC COM HPG MCS UCS MEC MAS LMAS MSW MFB SAW SDS TRN FD HELI SDCS`.
  - **If2Thirds (`max(k-1,1)` — "all but one", NOT literal 2/3)** flag: `AC3 PRB AECM ECM ENG LPRB LECM ORO RCN SRCH SHLD TAG WAT`.
  - **IfAll (`k`)** flag: `AMP BH EE FC SEAL MAG PAR RAIL RBT UMU`.
  - **Summed** (`Num` truncated to int, kept if `>0`): `SBF_OMNI CAR CK CT IT CRW DCC MDS MASH RSD VTM VTH VTS AT BOMB DT MT PT ST SCR PNT IF MHQ`.
  - Post: **FUEL** = min `u.get_fuel()` over aerospace units only; **CAR↔IT cancel**: `newCAR=max(CAR−IT,0)`, `newIT=max(IT−CAR,0)`, re-store both; **IF** → `Num` as ASDamage(value, false).
- **Tactics:** `tactics = max(0, 10 - movement + skill - 4)` = `max(0, 6 - movement + skill)`. Then if MHQ present: `tactics -= min(3, floor(MHQ/2))`. **No re-clamp after MHQ** (can go slightly negative; reproduce). `tactics` and `morale_rating` are not consumed by the to-hit calculation; `tactics` surfaces in the UI for the Step-5b damage-allocation check (see Known limitations: tactics/morale-rating consumers), and `morale_rating` is display-only.

### 2.5 Artillery damage table (`SBFFormation.java`) — `art_damage(code)->i64`

`ARTTC`1, `ARTT`2, `ARTBA`2, `ARTSC`2, `ARTAIS`3, `ARTAC`3, `ARTS`3, `ARTLTC`3, `ARTLT`6, `ARTCM5`8, `ARTCM7`13, `ARTCM9`22, `ARTCM12`36, default 0. Homing companion (`ARTAIS`/`ARTAC`→2, else 0) — **not** used by these converters; kept for §4 completeness.

### 2.6 Validation (golden vs MegaMek)

The converter is validated by golden tests that feed identical AS elements through neurohelmet's `convert_unit`/`convert_formation` and MegaMek's `SBFUnitConverter`/`SBFFormationConverter` and assert field-by-field equality; committed fixtures live under `data/sbf-goldens/` (with `data/sbf-goldens/input-parity.json` asserting that neurohelmet's baked `AsStats` matches the MegaMek `ASConverter` output for each fixtured chassis, so a derived-stat mismatch can never be a masked input skew). Because the golden asserts bit-identical output, the deliberately-reproduced MegaMek bugs — STL/MAS TMM `+2` never firing, ATAC ending at `sum/9`, the `art*` L-band excluding `artSC`, infantry `movement` below `rounded_average_move` while TMM keys off the higher value, the always-taken transport branch 3, and turret-only SUAs never reaching `SbfUnit.suas` (the Demolisher parity guard) — must be preserved, not fixed.

---

## 3. Session state and `GameMode::StrategicBattleForce`

This layer modifies `crates/core/src/domain.rs` (the enum) and `crates/core/src/session.rs` (state + methods). It depends on §1 & §2 (`sbf::{convert_unit, convert_formation, SbfUnit, SbfFormation, SbfRange}`, `as_element::as_element`).

### 3.1 The GameMode variant

```rust
pub enum GameMode { #[default] Classic, AlphaStrike, Override, StrategicBattleForce }
```
Three exhaustive matches cover the new variant: `point_cost` and `mech_cap` in `session.rs`, and the mode dispatch in `app.rs`/`view.rs` (§5). Because `GameMode` derives `Default=Classic` and `Serialize/Deserialize`, and the variant is appended, old sessions still deserialize.

### 3.2 Roster model — reuse `mechs`, add a grouping tree (mirror the Override pattern)

`Session.mechs: Vec<TrackedMech>` remains the **shared AS-element pool** (forcegen and `add_mech` already append here; each element carries its own `gunnery`=skill and `as_stats`). One field is added to `Session`:

```rust
// Session, append:
/// SBF grouping + live combat state. Empty/ignored unless `mode == StrategicBattleForce`.
#[serde(default)]
pub sbf: SbfState,
```
Add to `Session::default()` and `new_with_mode`: `sbf: SbfState::default()`.

**Two-sided model (paper trail — not shipped; see Scope).** The transcribed engine models exactly two sides — side `0` (yours) and side `1` (OpFor) — because initiative, turn interleave (§4.4–4.5) and target selection all require an opponent. Formations carry a `side`; per-side initiative lives on `SbfState`. The shipped single-force design drops `side`/`sides`/`SbfSideState` per the Scope deltas; the structures below are the two-sided reference.

```rust
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SbfState {
    pub formations: Vec<SbfFormationState>,   // BOTH sides; disambiguated by SbfFormationState.side
    #[serde(default)] pub active_formation: usize,
    #[serde(default)] pub active_unit: usize,   // within the active formation
    // Turn/round scaffolding:
    #[serde(default)] pub round: u32,
    /// Per-side state, indexed by `side` (0 = yours, 1 = OpFor). Holds this round's init roll.
    #[serde(default)] pub sides: [SbfSideState; 2],
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SbfSideState {
    /// This round's team 2d6 initiative total (§4.4). None until rolled.
    #[serde(default)] pub init_roll: Option<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SbfFormationState {
    pub name: String,
    /// 0 = your side, 1 = OpFor. Every combat/turn routine partitions `formations` by this.
    #[serde(default)] pub side: u8,
    pub units: Vec<SbfUnitState>,
    // ---- live formation combat state (derived stats are recomputed, never stored) ----
    #[serde(default)] pub morale: MoraleStatus,      // default NORMAL
    #[serde(default)] pub jump_used_this_turn: u8,   // count; §4.1 booleanizes via (>0)
    #[serde(default)] pub is_done: bool,
    // NOTE: `high_stress` is dropped — morale is a manual rung (§4.3); it was never implemented.
    // NOTE: initiative is NOT stored here — it lives in SbfState.sides[side].init_roll,
    // because SBF init is per-SIDE, not per-formation (§4.4).
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SbfUnitState {
    pub name: String,
    pub elements: Vec<usize>,          // indices into Session.mechs (the AS elements)
    // ---- live combat counters (the ONLY persisted unit slice) ----
    #[serde(default)] pub armor_hits: u16,   // current_armor = derived_armor - armor_hits (AS/Override convention)
    #[serde(default)] pub damage_crits: u8,
    #[serde(default)] pub targeting_crits: u8,
    #[serde(default)] pub mp_crits: u8,
}

/// Runtime morale ladder (SBFFormation.java). Ordinals 0→3; Routed terminal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoraleStatus { #[default] Normal, Shaken, Broken, Routed }   // Unsteady omitted: a MegaMek ACAR artifact, not an IO:BF rung (§4.3)
```

### 3.3 Deriving the live stat lines (recompute-on-demand, like `ov_card`)

Added to `impl Session` (or a thin `SbfState` impl). The AS-element pool → `AsElement` uses `TrackedMech.gunnery` for skill:

```rust
impl Session {
    /// Build the AsElement for pool index `i`.
    fn sbf_element(&self, i: usize) -> AsElement {
        let tm = &self.mechs[i];
        as_element::as_element(&tm.spec.as_stats, &tm.spec.display_name(), tm.gunnery)
    }
    /// Derived (immutable) SBFUnit for a unit state. Cheap; recomputed each frame like ov_card().
    pub fn sbf_unit(&self, u: &SbfUnitState) -> SbfUnit {
        let els: Vec<AsElement> = u.elements.iter().map(|&i| self.sbf_element(i)).collect();
        sbf::convert_unit(&u.name, &els)
    }
    /// Derived SBFFormation for a formation state.
    pub fn sbf_formation(&self, f: &SbfFormationState) -> SbfFormation {
        let units: Vec<SbfUnit> = f.units.iter().map(|u| self.sbf_unit(u)).collect();
        sbf::convert_formation(&f.name, &units)
    }
    /// Convenience: the resolved AsElements of a formation (needed by is_crippled §4.6).
    pub fn sbf_formation_elements(&self, f: &SbfFormationState) -> Vec<AsElement> {
        f.units.iter().flat_map(|u| u.elements.iter().map(|&i| self.sbf_element(i))).collect()
    }
}
```

### 3.4 Live-tracking methods on the state (the AS/Override analog — the interactive surface)

```rust
impl SbfUnitState {
    /// current armor = derived - hits (session recomputes derived via sbf_unit()).
    pub fn armor_remaining(&self, derived: &SbfUnit) -> i64 { (derived.armor - self.armor_hits as i64).max(0) }
    /// Apply SBF damage to this unit's armor pool (no per-element structure at unit scale). Fills
    /// armor, then RETURNS the overflow (damage that exceeded remaining armor) so the caller can
    /// spill it onto another unit — SBF damage carries over, it is not discarded (see §4.2 Spillover).
    /// `apply_damage(d) == 0` means it was fully absorbed. Negative input returns 0.
    pub fn apply_damage(&mut self, derived: &SbfUnit, dmg: i64) -> i64 { /* fill; return dmg - absorbed */ }
    pub fn repair(&mut self, n: i64);                 // armor_hits = armor_hits.saturating_sub(n)
    pub fn add_damage_crit(&mut self);  pub fn add_targeting_crit(&mut self);  pub fn add_mp_crit(&mut self);
    /// getCurrentDamage = damage.reducedBy(damage_crits): each crit −1 at EVERY band, floored 0.
    /// Returns the full reduced vector; §4.2 picks a band via `.band(range)`.
    pub fn current_damage(&self, derived: &SbfUnit) -> DamageVector { reduced_by(derived.damage, self.damage_crits) }
    /// getBaseGunnery = skill + targeting_crits (SBFUnit.java).
    pub fn base_gunnery(&self, derived: &SbfUnit) -> i64 { derived.skill + self.targeting_crits as i64 }
    /// destroyed iff no armor remains (§4.2).
    pub fn is_destroyed(&self, derived: &SbfUnit) -> bool { self.armor_remaining(derived) == 0 }
}

impl DamageVector {
    /// Band selector for a firing range (§4.2). Non-As vectors have no E band → 0 at Extreme.
    pub fn band(&self, r: SbfRange) -> f32 {
        match r {
            SbfRange::Short   => self.s,
            SbfRange::Medium  => self.m,
            SbfRange::Long    => self.l.unwrap_or(0.0),
            SbfRange::Extreme => self.e.unwrap_or(0.0),  // 0.0 for every ground (non-As) unit
        }
    }
}
```
`reduced_by(v, n)` (mirror `ASDamageVector.reducedBy`, `ASDamageVector.java`): subtract `n` from **each** band, floor 0 (`5/2/1 reducedBy 1 → 4/1/0`; `reducedBy 3 → 2/0/0`).

### 3.5 Roster/cap/point-cost integration

- `mech_cap`: `StrategicBattleForce => None` (uncapped, like AlphaStrike; a Company is 12+ elements per side).
- `point_cost`: SBF uses **AS PV summed into formation PV**. Per-element pool cost = `skill_adjusted_pv(pv, gunnery)` (same as AlphaStrike). `force_total` continues to sum `point_cost` over `mechs`. SBF PV is `round(basePV/3)` per unit then skill-scaled at the unit's averaged skill, which is *not* identical to summing per-element skill-adjusted AS PV. Accordingly the UI displays the **derived formation PV** (`sbf_formation(f).point_value`) as the SBF force cost, and keeps `force_total` (element AS PV sum) only as a budgeting proxy for forcegen. This discrepancy is noted in Known limitations (SBF PV vs summed AS PV).
- **Grouping ops:** `Session` methods to (a) create a formation from a contiguous run of pool elements auto-split into units of ≤6, (b) move an element between units, (c) rename, (d) manage empties. Because forcegen already emits Lance/Star/Level II/Company sized draws (`forcegen::FORMATIONS`), the natural default when generating an SBF force is: 1 formation split into `ceil(n/6)` units of ≤6 elements each, respecting the 1–4-units / ≤6-elements-per-unit / ≤20-elements-total structural caps (`BaseFormationConverter.java`). (The two-sided paper trail carried a `side: u8` argument on these signatures, e.g. `fn sbf_new_formation(&mut self, side: u8, name: &str, pool_range: Range<usize>)`; the shipped single-force design drops it.)

### 3.6 Structural validity (`can_convert`, `BaseFormationConverter.java`, IO:BF p.163) — enforce on grouping

A formation grouping is valid iff: 1–4 units; ≤20 total elements; 1–6 elements per unit; no element with `LG` in a unit of >2 elements; no `VLG`/`SLG` in a unit of >1; a ground unit contains no aerospace element; an aerospace unit's ground elements all have `SOA`/`LAM`/`BIM`. Violations surface as a non-blocking warning in the UI (a hand-built force is not hard-rejected; SBF players sometimes bend these), but the "convert" action is blocked from producing nonsense.

---

## 4. Combat resolution: to-hit, damage/crits, morale, initiative/turns

Combat lives in `crates/core/src/engine/sbf.rs` (a `combat` submodule) plus turn/round helpers on `SbfState` in `session.rs`. It depends on §1–3.

SBF in neurohelmet is a **manual combat tracker** (like the AS/Override modes): the player marks damage/crits/heat/morale by hand; the engine computes to-hit target numbers and derived effects. There is **no board** — neurohelmet has no hex map — so board-distance range brackets, LOS, stacking, movement pathing and the whole detection/visibility layer are **out of scope** (they gate targeting via the server's data layer, not the to-hit number). The player enters range bracket and (optionally) the target formation by hand, exactly as the existing AS `AsTarget`/Classic `CtTarget` editors do.

**Opponent model.** The single-force tracker always hand-enters the target's legs, mirroring `AsTarget`/`OvShot`; only the target's TMM/morale is needed to roll to-hit. The transcribed engine also supports a tracked-OpFor path (populating the to-hit context from a side-1 formation's derived TMM + live morale); that path — the `with_target` constructor below — is **paper trail, not built** (single-force scope, see the Scope deltas). The same `SbfToHitCtx` fields serve both; only the *source* of the numbers differs.

```rust
impl SbfToHitCtx {
    /// (Paper trail — NOT built.) Populate the target legs from a tracked opposing formation.
    pub fn with_target(mut self, session: &Session, target: &SbfFormationState) -> Self {
        self.target_tmm = session.sbf_formation(target).tmm;
        self.target_jumped = target.jump_used_this_turn > 0;   // booleanize the stored count
        self
    }
}
```

### 4.1 To-hit — `pub fn sbf_to_hit(atk: &SbfToHitCtx) -> SbfToHit`

MegaMek has **two divergent implementations**; both were transcribed. neurohelmet implements the **printed IO:BF p.172 To-Hit Modifiers Table** as the authoritative source (superseding both MegaMek transcriptions). Base = the firing formation's **Skill** (`SBFToHitData.java`). MegaMek's mainline does **not** apply targeting crits (`getBaseGunnery` has zero callers); the ACAR path does. Because neurohelmet fires at unit granularity, its default **does** add the firing unit's `targeting_crits` (the useful behavior, matching ACAR `AttackToHitData`). This divergence is documented in Known limitations (targeting-crit gunnery penalty).

Input context (hand-entered; note there is **no morale field**, see below; the calculation follows the printed p.172 table):
```rust
pub struct SbfToHitCtx {
    pub attacker_skill: i64,          // firing FORMATION skill (base)
    pub firing_unit_targeting_crits: u8, // +1 per crit
    pub range: SbfRange,              // S +0 / M +1 / L +2 / E +3 — player-chosen bracket
    pub indirect_fire: bool,          // additional +1 (L/E attacks)
    pub attacker_jump: u8,            // = formation.jump_used_this_turn, +1 PER POINT
    pub withheld_units: u8,           // −1 each, floored at −2
    pub bfc: bool, pub drone: bool,   // derived from formation SUAs (BFC / DRO), +1 each
    pub spotting: bool,               // spotting for IF this turn, +1
    pub secondary: bool,              // secondary target, +1
    pub target_tmm: i64,              // >0 only adds — hand-entered
    pub target_jump: u8,              // +1 per point — hand-entered
    pub target_evaded: bool,          // +1
    pub terrain: i64,                 // hand-entered aggregate (woods/urban/underwater)
}
pub fn sbf_to_hit(&SbfToHitCtx) -> i64   // hit iff 2d6 >= n; no Impossible arm (Extreme is legal)
```
**Jump is a count, not a flag.** `jump_used_this_turn: u8` feeds the to-hit as-is: the printed table (p.172) is **+1 per JUMP point used**, for both attacker and target.

Computation — the printed To-Hit Modifiers Table, **IO:BF p.172**:
```
base = attacker_skill                            // formation Skill; each firing unit rolls 2d6 vs the number
+ firing_unit_targeting_crits                    // "+1 per Critical" (attacker table)
+ range_mod(range)                               // S +0 / M +1 / L +2 / E +3 — Extreme is a LEGAL attack
+ if indirect_fire { 1 }                         // additional +1 on the range mod (L/E attacks only)
+ attacker_jump                                  // +1 PER JUMP POINT used (not flat)
- min(withheld_units, 2)                         // withholding fire: −1 per unit, max −2
+ if bfc { 1 } + if drone { 1 }                  // formation specials (derived from SUAs, not entered)
+ if spotting { 1 }                              // also spotting for IF this turn
+ if secondary { 1 }                             // secondary target
+ if target_tmm > 0 { target_tmm }               // negative TMM ignored
+ target_jump                                    // +1 PER JUMP POINT used by the target
+ if target_evaded { 1 }                         // successful Evade
+ terrain                                        // hand-entered (woods +1/+2, urban +1/+2, underwater +1)
                                                 // morale NOT applied — manual rung (§4.3); the printed
                                                 // −1/−2/−3 demoralized-target row is a deferred decision
```
Deliberately omitted from the ctx (too niche; hand-add via terrain if ever needed): artillery-attack-this-turn (+1/ART) and dismounted-from-transport (+1). Resolution: **hit iff `2d6 >= number`** — Step 4 says "equals or exceeds"; there is **no natural-2/12 auto rule**.

**Morale is not in the default calc.** Morale is a **manual rung** in neurohelmet (§4.3) — the player sets Normal/Shaken/Broken/Routed by hand; the tracker does not simulate checks. Consistent with that, the target's morale does **not** feed the to-hit number: `SbfToHitCtx` carries **no morale field at all** (the rung is display-only, shown by the target editor, never consumed by `sbf_to_hit`), so morale-invariance of the number holds by construction. Wiring the demoralized-target modifier into the calculator is a **separate, deferred decision**; the rulebook prose (IO:BF p.175) is a **−1/−2/−3 to-hit bonus** for attacking a Shaken/Broken/Routed target (demoralized = *easier* to hit — the opposite sign from the p.170 Engagement-Control roll's +1/+2/+3, which neurohelmet does not model at all). See Known limitations (demoralized-target modifier).

**Range semantics (p.172):** the brackets are attack *declarations*, not measured inches — S and M are same-hex attacks, L reaches the same or an adjacent hex, E reaches two hexes; the player picks the bracket by hand. MegaMek's hex stub (0/1/2 = +0/+3/+3) and the ACAR ladder (−1/+2/+4/impossible) are both wrong against the book and are not used anywhere.

### 4.2 Damage application + crit table — `sbf_crit(roll)` / `apply_crit` / `crit_check_due`

*(There is deliberately no monolithic `resolve_hit`: the tracker is manual, so the surface is three small pieces — `crit_check_due` says whether a crit roll is owed, the player rolls, `sbf::sbf_crit(total)` reads the table, `apply_crit` marks the result. Damage marking is `apply_damage` + the spillover chain below.)*

neurohelmet applies damage at unit scale (the AS/Override convention: one aggregate armor pool per SBFUnit; neurohelmet does **not** model per-element structure at SBF scale). The **ACAR reference** governs, not the interactive stub (`StandardUnitAttackHandler.java` in `server/sbf` is a non-authoritative stub — do not port).

On a hit, the damage dealt is the **band of the firing unit's post-crit damage vector at the firing range**:
```
let derived = session.sbf_unit(firing_unit_state);
let dmg: f32 = firing_unit_state.current_damage(&derived).band(range);   // §3.4: reduced-by-crits, then band-select
```
`current_damage(&derived) -> DamageVector` returns the full crit-reduced vector (§3.4), and `DamageVector::band(range)` selects the S/M/L/E entry. **Extreme is a legal attack (+3, p.172)**; a vector without an E band deals **`L − 1`** there (Step 5a: "Extreme Range damage is equal to Long Range −1"), floored at 0 — an explicit E band (aerospace) wins over the fallback.

**Spillover — SBF damage carries over, it is not discarded.** The player marks the damage on a chosen target unit; `apply_damage` (§3.4) fills that unit's armor and **returns the overflow**. Any overflow is then placed by the player on **another unit of the target formation**, exactly like a TW arm→torso carry-over, and this **chains** — if the next unit also caps, its overflow spills again — until the remainder is 0 or the formation has no unit left to absorb it. This is player-controlled allocation (faithful to SBF's attacker-allocates model), not an auto-target; the engine only computes each unit's overflow.

**Timing — apply immediately.** When the player marks damage or a crit, neurohelmet applies it to state **right then**, exactly as Classic/AS/Override modes do — no "pending → commit at End Phase" staging. Simultaneity of fire and return-fire is a tabletop concern the players resolve at the table; the tracker just records the marks as they are made. (Crits are still *rolled* conceptually in the Combat phase and their board-game effects "take effect" at End Phase, but for a tracker that distinction collapses to apply-on-mark.)

**Crit check trigger** (ACAR `StandardUnitAttackHandler`): roll a crit **iff `current_armor*2 < full_armor`** (current below half full). Then `2d6` → table (`handleCrits`):

| 2d6 | Effect |
|---|---|
| 2,3,4 | no crit |
| 5,6,7 | `add_targeting_crit` (+1 to-hit, permanent) |
| 8,9 | `add_damage_crit` (−1 damage all bands) |
| 10,11 | targeting **and** damage |
| 12 | **unit destroyed** (`current_armor := 0`) |

**One table.** IO:BF uses a **single** SBF crit table — the per-unit-type columns on IO p.87 are a Standard-BattleForce copy-paste, not SBF. So this ACAR split (2-4 none / 5-7 Targeting / 8-9 Weapon-damage / 10-11 both / 12 destroyed) is the table neurohelmet implements. `add_mp_crit` exists but no roll on this table generates an MP crit — MP crits are a **manual mark only** (e.g. the player reading a Movement result off the physical table); neurohelmet does not roll them.

**Crit floors.**
- **Damage crits floor at 0**, never negative: `current_damage` = `reduced_by(damage, damage_crits)` subtracts 1 per crit at every band and floors each band at 0 (§3.4). A unit at all-zero damage simply does no damage.
- **MP crits floor at 0 → immobile.** Each MP crit is −1 MP, floored at 0. There is **no minimum-move floor of 1**: IO:BF's "Minimum Movement" rule (p.166) only lets a unit that *has* ≥1 MP still move 1"; it does not keep MP from reaching 0. So Movement crits can drive a unit to 0 MP / immobile. (Display concern only in this tracker — MV is shown, not pathed.)

**Air-to-ground "Crew/FCS Hit" = Targeting Damage.** The poorly-worded air-to-ground "Crew/FCS Hit" crit maps to a **Targeting** crit (`add_targeting_crit`), same as a 5-7 result.

**Environmental / underwater crits are out of scope.** IO:BF's underwater rule (p.173: a unit underwater rolls on the crit table *twice*) and other environmental modifiers are **not** modeled — a tracker should not interrogate the player about vacuum/water/atmosphere each turn. neurohelmet rolls the single table once.

**Destruction & formation rollup** (ACAR `EndPhase`): a unit is destroyed at `current_armor <= 0` or crit-12. neurohelmet marks the unit destroyed (`SbfUnitState::is_destroyed(derived)`: `armor_remaining==0`) and, per `EndPhase.destroyUnits`, **a formation is eliminated when its last unit is destroyed** — surfaced as a "formation eliminated" state, with removal left to the player (tracker, not sim).

### 4.3 Morale: a manual rung, not a simulation

**Morale is player-set, full stop.** neurohelmet does **not** simulate morale: no check target numbers, no pass/fail rolls, no recovery rolls, no automatic triggers, no auto-effects. The formation's `morale: MoraleStatus` (§3.2, four rungs `Normal/Shaken/Broken/Routed`) is a **settable label** the player advances or recovers by hand, reading it off their own record sheet exactly as at the table.

Rationale: the full IO:BF morale system (Engagement Control, check TNs, nerve recovery, forced withdrawal) is a large, self-contained subsystem, and MegaMek's two implementations disagree on nearly every number (TN formula, recovery direction, to-hit sign). Rather than bake a contested simulation into a tracker — and stall the *meat*, movement and combat — neurohelmet ships morale as a rung and treats auto-morale as a separate future project. This keeps the tracker faithful to physical play (where the player owns the morale track) and side-steps every NEEDS-RULEBOOK morale contradiction.

**Consequences for the implementation:**
- No `high_stress` machinery. The four ACAR high-stress triggers (armor halved by one hit, crippled-and-still-armored, unit destroyed, crit-12) are **not** wired up; nothing sets or reads `high_stress`, and the field is absent from `SbfFormationState` (serde-safe, since unknown keys are ignored on load).
- No morale-check TN, no `2·skill−2` vs `3+skill` choice, no pass/fail roll.
- No nerve-recovery roll (and thus no recovery-inversion to worry about).
- No morale term in the to-hit calc (§4.1) — the target-morale modifier is a **separate deferred decision**.
- No auto forced-withdrawal. A `Routed` or crippled formation is simply **flagged** in the UI ("would withdraw — BA/CI exempt"); the player decides. This is the one place the rung and §4.6 crippling surface a hint, but it triggers nothing.

**The interactive surface** is therefore just: `m` cycles/sets the rung (`Normal → Shaken → Broken → Routed` and back). No dice.

*The full ACAR morale machinery — check TNs, the `3+skill` vs `2·skill−2` dispute, the recovery-direction inversion, the high-stress triggers, forced withdrawal — is preserved verbatim in [Appendix A](#appendix-a--morale-simulation-reference-not-implemented) as the reference for a possible future auto-morale project. It is not part of the shipped combat layer.*

### 4.4 Initiative (paper trail — not built; single-force, see Scope and §4.5)

Plain **team/side 2d6** (`InitiativeRoll` rolls `d6(2)`; MegaMek SBF passes init-compensation `false` and no Combat-Sense, `SBFGameManager`). **Higher total wins; the loser (lowest) acts first** (ascending turn sort — BattleTech "loser moves first"). Ties: re-roll only among the tied, append the extra roll, compare element-by-element (`TurnOrdered`). In the two-sided model each side gets one roll per round: `roll_initiative` writes `SbfState.sides[0].init_roll` and `sides[1].init_roll` (each a 2d6 total), re-rolling ties. Init is stored per side on `SbfState.sides`, never on individual formations. The round increments at initiative (`SBFPhasePreparationManager` — round 0 = setup).

### 4.5 Turn order / activation

**Two-sided interleave (paper trail — not built).** The primary numeric rule (`SBFInitiativeHelper.determineTurnOrder`): the side with more formations activates several per segment (the **movement ratio**), so both sides finish together, and within a segment the **initiative loser goes first**. Sides are ordered by `SbfState.sides[side].init_roll` **ascending** (loser first):
```
lowest = min over sides of (# eligible formations)
for segment in 0..lowest:
    cur_lowest = min over sides of remaining_count           // recomputed each segment
    for side in [0,1] sorted ASCENDING by sides[side].init_roll (loser first):
        move_n = remaining[side] / cur_lowest                // INTEGER division (truncate)
        move_n = min(move_n, remaining[side])
        activate move_n formations of side; remaining[side] -= move_n
```
Firing phase is simpler: one turn per eligible formation, sorted ascending by side init. A formation is eligible iff `!is_done` and (in normal phases) deployed (`SBFFormation.isEligibleForPhase`). `is_done` resets each phase/turn (`resetEntityPhase` sets all `false`); `jump_used_this_turn` is **not** reset by phase-prep (it persists into firing for TMM, `SBFMovementProcessor`) and is the count §4.1 reads.

**Single-force scope (what is actually built).** The two-sided initiative (§4.4) and unequal-numbers interleave (above) are **out of scope** — neurohelmet tracks only *your* formations, so there is no opponent side to interleave against and no `roll_initiative`/`turn_order`. What single-force keeps is a lightweight turn/round tracker over your own formations:
- `SbfState::begin_round()` — increment `round`, clear every formation's `is_done`, reset `jump_used_this_turn` at the movement-phase boundary. (No initiative roll.)
- `SbfState::begin_phase()` — clear `is_done` across all formations for the new phase.
- `SbfState::end_turn(formation_idx)` — set that formation's `is_done = true`.
- `roll_initiative` / `turn_order` — **not built** (two-sided; out of scope).

### 4.6 Crippling test — `is_crippled(&self, f: &SbfFormationState, session: &Session) -> bool` (`Formation.java`, IO BETA p.242)

**`is_crippled` needs the AS element pool**, because the rulebook thresholds are counted at the ELEMENT level inside each unit — so the method takes `&Session` (to resolve elements via `session.sbf_formation_elements(f)` / `session.sbf_unit(u)`), not just the formation. neurohelmet tracks live damage at UNIT scale, so tests operate on per-element *base* stats reduced by the containing unit's live crit/armor counters, with the per-element-structure gap explicitly approximated.

Crippled if **any** (all thresholds `ceil(n/2.0)`):
1. **≥ half of elements do zero damage.** For each element, take its base standard-damage vector reduced by its **containing unit's `damage_crits`** (`element_std_damage.reduced_by(unit.damage_crits)`); an element counts if it *had* damage originally but the reduced vector is now all-zero. Crippled if `count >= ceil(total_elements / 2)`. (`total_elements` = sum of `unit.elements.len()` across the formation.)
2. **≥ half of armored elements gutted.** *Rulebook:* an element is gutted if `current_armor==0 && (current_structure < full_structure/2 || full_structure==1)`, excluding CI/BA. **neurohelmet approximation (pinned, not left vague):** since neurohelmet has no per-element structure at SBF scale, count at the **unit** level — a unit is "gutted" iff `armor_remaining(derived)==0`; count `units_with_armor` = units whose derived `armor > 0` and whose `sbf_type ∉ {Ci,Ba}`; crippled if `#{gutted, non-infantry units} >= ceil(units_with_armor / 2)`. This substitutes `unit.armor_remaining==0` for the per-element `current_armor==0 && structure<half` test; being exact requires per-element structure (see Known limitations: per-element structure).
3. **≥ half of units have ≥2 targeting crits.** Crippled if `#{u : u.targeting_crits >= 2} >= ceil(units.len() / 2)`.

---

## 5. TUI (`crates/app`)

The TUI modifies `crates/app/src/tui/{app.rs, view.rs}` and reuses `forcegen.rs`. It mirrors the existing Override screen throughout.

### 5.1 New session flow (`app.rs`)

- `Screen` enum: add `Sbf`.
- Mode→screen matches: `StrategicBattleForce => Screen::Sbf`.
- Key dispatch: `Screen::Sbf => self.sbf_key(key)`.
- New-session modal (`PendingAction::NewSession(GameMode)`): add a fourth choice "Strategic BattleForce" → `NewSession(GameMode::StrategicBattleForce)`.
- Point-unit label matches (in `app.rs` and `view.rs`): add the SBF arm → label "PV", short tag e.g. `"SF"`; `as_destroyed`/damaged predicates route to `sbf_unit(...).armor_remaining==0` / `armor_hits>0`.

### 5.2 The SBF screen (`view.rs`) — three nested panes, single force

The layout is modeled on the AS card + Override card renderers (`as_card_lines`, the `ov_card` view). Single force: the list holds only your formations. **No force sidebar on this screen:** its roster cursor cannot be moved here and the detail pane lists the active unit's elements (with pool numbers matching the group editor), so a sidebar would only duplicate that with a dead highlight; the force PV total it would carry lives in the FORMATIONS pane title instead. Panes:
1. **Formation list** (single-force — only *your* formations; no side grouping, no `init_roll` row): name, type, `SZ/TMM/MV/T(actics)/M(orale rating)/PV@skill`, live `morale` **rung** glyph (player-set), per-formation `is_done`/jump flags. `,`/`.` cycle formations (reuse the existing unit-cycle keybinds).
2. **Unit list** (units in the active formation): each unit's derived `SZ/MV/A/damage/PV@skill/specials` (from `sbf_unit`) plus **live** `armor_remaining` (derived armor minus `armor_hits`), `damage_crits`/`targeting_crits`/`mp_crits` counters, current (post-crit) damage. Selectable; `active_unit`.
3. **Detail / actions** for the active unit + formation-level morale/turn state.

### 5.3 Keybindings (reuse the AS/Override verbs; keep the `?` modal + cheat sheet in sync)

Reuse the established verbs so muscle memory transfers (the AS help and Override help modals in `view.rs`):
- `Space` / `u` — apply / repair one point of SBF damage to the active unit (`apply_damage`/`repair`). When a marked packet exceeds the unit's armor, the **overflow spills over**: prompt the player to place the remainder on another unit (chaining until absorbed), per §4.2 spillover.
- `c` — crit popup: mark Damage/Targeting/MP crit (`add_*_crit`), matching the AS crit modal (`as_crit_modal_lines`). (MP crit is a manual mark; the crit *roll* never produces one — §4.2.)
- `t` — to-hit shot editor: range bracket, attacker-jumped, secondary/#targets, and hand-enter target TMM/target-jumped → live per-range to-hit preview (mirror the `as_to_hit` editor). Target morale is shown for reference but **does not change the number** (§4.1, manual morale).
- `m` — morale: cycle/set the formation `MoraleStatus` rung (`Normal→Shaken→Broken→Routed` and back). **No roll, no TN** — it is a pure label (§4.3).
- `n` — new round: `begin_round` (increment `round`, clear `is_done`, reset jump). `e` — end this formation's turn (`is_done=true`). **No initiative / interleave** (single-force; out of scope).
- `a` — add element to the pool (reuse the picker). **Grouping is manual-first:** `g` opens the *grouping editor* (`Modal::SbfGroup`): every pool element listed with its `formation · unit` assignment (opening on the sidebar-highlighted element); `↑↓` select, `←→` move between grouping stops (every unit, plus a virtual "new unit" stop for each **empty formation**), `n` split into a new unit of its formation, `f` start a new formation, `u` unassign; each move is one undo step. **Empty-group lifecycle:** empty *formations* are first-class workspaces — new SBF sessions seed an empty "Formation 1", vacating a formation leaves it standing as a `(no units)` placeholder (selectable as a grouping stop), and only `D` removes a formation. Empty *units* stay alive as move targets while the editor is open and come off the sheet when it closes. Empties render as placeholders — never as destroyed/eliminated — and contribute no PV, no withdrawal hint, no elimination state. **Auto-group is the opt-in** `a` inside the editor → `Modal::SbfDoctrine`, an IO:BF p.165 doctrine picker (`Session::sbf_group_doctrine`): Inner Sphere (Lances of 4 → Companies of 3), Clan (Stars of 5 → Binary/Trinary, named by unit count), ComStar (Level IIs of 6 → Level III, capped at 3 by the ≤20-element rule); **ground and aerospace are never mixed** (aero groups as Flights of 2 → Squadrons; ComStar Level IIs) — the `can_convert` type rule enforced by construction. Doctrine regroup replaces all formations and clears live marks — a **pristine** grouping applies instantly (group-first stays frictionless), while anything hand-entered triggers an **itemized confirmation** ("Discards 2 custom name(s), 5 armor hit(s), the COM mark — z undoes"; heuristic name detection via `sbf_default_name`); either way it is one undo step. `r` renames the active formation. No `side` handling (single-force).
- `1` — omitted (SBF is inch-native; no ground-scale toggle).
- `L` — **game log:** `LogEntry` carries `#[serde(default)] sbf: SbfState` alongside the element pool, so each entry is a self-contained snapshot of grouping + live formation state. On export, `render_turn` renders an SBF entry as **one formation-sheet frame per formation** (the three-pane screen with that formation active), headed by the formation name; old SBF log lines without the field deserialize to an empty `SbfState` and fall back to per-element AS cards. Non-SBF modes are unchanged.
- `S` — sessions; `D` — delete the **active formation** (confirm; removes its pool elements too); `b` — force PV limit; `z` — undo.
- **COM/LEAD (IO:BF p.165/p.172):** `C` toggles the active unit as **Force Commander** (COM — at most one across the force; the formation list shows the inherited COM badge), `l` toggles it as **Formation Leader** (LEAD — at most one per formation). Tracked as designations (`SbfUnitState.is_commander`/`is_leader`, serde-default); their mechanical role is the **Step-5b damage-allocation Tactics check** (winner picks which *Unit* takes the damage; a defending formation holding COM/LEAD rolls at +2) — a cross-player table roll, so the detail pane shows a "defender +2 Tactics (COM/LEAD)" hint rather than rolling it. `R` renames the active unit. In the group editor: `s`/`S` adjust the selected element's Skill (drives unit/formation skill + PV), `x` removes it from the force.

An SBF `?` help modal (`sbf_help_modal_lines`) lists all of the above. Any new SBF keybinding is kept in sync across the in-app `?` modals in `view.rs`, `docs/keybindings-cheatsheet.html`, and the committed cheat-sheet PDF.

---

## Strategic Aerospace layer (SAS, IO:BF pp.177–181)

The SBF chapter's aerospace sibling. Page cites are extraction markers (printed = marker − 2). The section splits cleanly under the single-force boardless lens:

**Table-concern (out of scope):** the Atmospheric Radar Map (rings/zones/stacking/placement, p.177–178), zone movement (Thrust 1–5/6–10/11+ → 1/2/3 zones, p.178), engagement control + tailing determination (opposed maneuver rolls, p.179) and ending engagements (p.181) — all two-player positional procedure. The engagement modifiers (+2 atmosphere, +2 tailed, −2 tailing) render as dim reference text only. Landing/liftoff and everything "Advanced Strategic Aerospace" (pp.182–196: large-craft squadrons, capital map, forced landings) stays unread/unscoped.

**Tracker-core (in scope):**

1. **Flight/Squadron structure (p.177).** Aero Units are *Flights* = **2 elements** (fighters/CF/small craft); aero Formations are *Squadrons* = **2–6 Flights** (2–12 elements). `sbf_group_doctrine`'s aero arm pairs Flights of 2 → Squadrons; `sbf_can_convert` has an aero branch: formations whose units are all-aero validate at **≤6 units / ≤12 elements / ≤2 elements per Flight**, replacing the ground 4/20/6 caps. (Airship Flights = 1 element — unreachable: airship SVs type to `V`/ground in the baked data; noted, not built.)
2. **The Aerospace To-Hit Modifiers table (p.179), verbatim** — extends `SbfToHitCtx`/`sbf_to_hit` with an `aero: Option<SbfAeroShot>` leg:
   - Range: S +0 / M +1 / L +2 / E +3 (identical to the ground p.172 row — no new range code).
   - Target type: airborne aerospace +2 (*only if the attacker is not itself an airborne aerospace Squadron*, fn); airborne DropShip −2; airborne VTOL/WiGE +1; Small Craft −1.
   - A2G attack type: Altitude Bombing +3 / Dive Bombing +2 / **Strafing +4** / Striking +2 / Cluster Bomb −1. (Note Strafing is +4 here vs Standard-BF's +2 — different scale, both correct.)
   - Misc: Drone +1; grounded DropShip −2; attacker "behind" the target −2; SV fire control AFC +0 / BFC +1 / neither +2; Targeting crit +2 each (multiple).
   - **Air-to-air applies no target-movement and no terrain modifiers** (p.179) — the ctx's `target_tmm`/`target_jump`/`terrain` legs are suppressed when the aero leg is an air-to-air kind. Ground-to-air likewise: flat +2 airborne, no TMM (p.181); grounded Squadron = −4 immobile, no TMM (p.181).
3. **A2G damage math (p.180):** Strafing = **¼ of the Flight's Short value, round up**, up to 4 formations along the path, per-Flight rolls; Striking = full Short value, one Formation; Altitude Bombing = per-hex rolls (≥1 BOMB point per hex; BA may not); Dive Bombing = per-5-bombs rolls vs one Formation; HE bomb = 2 damage per bomb attack, Cluster = 1 (with the −1 to-hit row). Mixed aerodyne+spheroid Squadrons: Striking and Altitude Bombing only. Bombing applies no target movement/type/terrain mods (p.180; see Known limitations: SAS A2G target-modifier for the strafe/strike TMM contradiction).
4. **Thrust loss (p.178/p.181):** an airborne aero unit at 0 movement (MP crits) is **crashed — destroyed in the End Phase**, not immobile: aero units at movement 0 render a `CRASHES (End Phase)` badge in place of the ground immobile state; the mark stays manual (apply-immediately doctrine unchanged). BOMB-carrying thrust reduction (−1/bomb, min 1, p.178) is a card note, not tracked state (see Known limitations: SAS bombs-carried).
5. **Attack structure (p.179):** one attack per Flight in the Squadron — already SBF's per-Unit attack model; no change.

SAS lives in `engine/sbf.rs` (`SbfAeroShot` kinds + rows in `sbf_to_hit`, strafe/bomb damage helpers), `session.rs` (`sbf_can_convert` aero branch), and `app.rs`/`view.rs` (shot-modal aero rows, crash badge, help/cheatsheet).

---

## Known limitations and rulebook gaps

Items below are unresolved in MegaMek (stub, `//TODO`, dead code, or two implementations that disagree) or are deliberate neurohelmet divergences. Definitive resolution requires **Interstellar Operations: BattleForce**.

- **Transport-movement "only some elements transportable" branch.** A literal `[TBD]` stub in `SBFUnitConverter`; its selecting test (`getCAR() <= getIT()`) reads unit SUAs that are still 0, so it always takes branch 3. Reproduced as-is (§2.3 step 6). **NEEDS RULEBOOK.**
- **STL/MAS TMM +2.** Reads unit SUAs before they are populated, so it never fires; reproduced as dead (§2.3 step 7). The intended STL/MAS TMM bonus is unconfirmed. **NEEDS RULEBOOK.**
- **ATAC double-division.** Net divisor ≈ `sum/9` (§2.3 step 9o). Confirm the intended divisor. **NEEDS RULEBOOK.**
- **STL/MAS/LMAS mixing.** A class `//TODO`: if elements mix STL/MAS/LMAS, `calcSTL` grants all three (§2.3 step 9i). **NEEDS RULEBOOK.**
- **Targeting-crit gunnery penalty.** MegaMek's mainline never applies it (`getBaseGunnery` has no callers); ACAR does. neurohelmet adds it by default (matching ACAR, §4.1). Confirm that crits worsen gunnery. **NEEDS RULEBOOK.**
- **Demoralized-target to-hit modifier.** IO:BF p.175 gives a −1/−2/−3 to-hit bonus for attacking a Shaken/Broken/Routed target (demoralized = easier to hit; distinct from the p.170 Engagement-Control +1/+2/+3, which neurohelmet does not model). Because morale is a manual rung (§4.3), this modifier is **not applied** by default; wiring it into `sbf_to_hit` is a deferred decision.
- **Turret abilities excluded from conversion (deliberate MegaMek parity).** The parser routes `TUR(…)` SUAs into `turret_suas` and the converter ignores them, matching MegaMek's top-level-only `hasSUA` so the golden holds (trap #1). The rules-correct behavior is almost certainly that turret weapons contribute IF/FLK/ART/AC/etc. to the SBFUnit. If IO:BF confirms this, the fix belongs in the combat/conversion-v2 layer and the golden must migrate from "match MegaMek" to hand-authored expected values. **NEEDS RULEBOOK.**
- **SBF PV vs summed AS PV.** Formation PV (`sum over units of round(basePV/3)` then skill-scaled) is not identical to summing per-element skill-adjusted AS PV (§3.5). The UI shows the derived formation PV; `force_total` remains an AS-PV budgeting proxy. Confirm the canonical SBF force-cost. **NEEDS RULEBOOK.**
- **Per-element structure at SBF scale.** The crippling "gutted armored elements" test (§4.6 condition 2) and the ACAR armor-then-structure spill need per-element structure, which neurohelmet does not track at unit scale. neurohelmet approximates with unit-scale `armor_remaining==0` over `ceil(units_with_armor/2)`. Modeling per-element structure would make it exact. **NEEDS RULEBOOK.**
- **Tactics / morale-rating consumers.** `tactics` feeds the Step-5b damage-allocation Tactics check (IO:BF p.172): when hits land, both players roll 1D6 (the better-Tactics side adds the difference; ties → defender) and the winner chooses which Unit of the target formation takes the damage; a defending formation holding COM or LEAD adds +2. This is a cross-player table roll, so the tracker surfaces TAC and shows a "defender +2 Tactics (COM/LEAD)" hint rather than rolling it. `morale_rating` is display-only (manual morale). Concentration of Fire (max 2 damage-events per unit per exchange unless all are at 2) and Simplified random allocation (1D6) are left to the table.
- **SAS A2G target-modifier contradiction (p.180).** The *Targeting* paragraph says air-to-ground to-hit rolls are made "as if they targeted a ground hex" with "a Formation's TMM … not factored in" unconditionally, while Step 3 says only *bombing* attacks skip the target's movement/type/terrain. neurohelmet **follows Step 3** (bombing excludes; strafe/strike include) as the more specific rule matching Standard BF's shape. **NEEDS RULEBOOK ERRATA.**
- **SAS bombs-carried state.** Carrying bombs costs 1 Thrust each (min 1, p.178) and gates the bombing attack types, but load-out is scenario setup the sheet does not track; it renders as a card note when the formation has BOMB. A per-formation counter is added only if play-testing wants it.
- **Detection / visibility & the sensor table.** MegaMek's `sensorDetectionResult` table never yields BLIP/I_GOT_SOMETHING/EYES_ON_TARGET/PARTIAL_SCAN_RECON though those states exist; visual detection is dead code (`SBFDetectionHelper`). Out of scope for a boardless tracker, but the reveal semantics are undefined. **NEEDS RULEBOOK.**
- **neurohelmet data gaps.** `isAerospaceSV` (SV→AS vs V), PM/MS/large-craft AS types, and unit quirks (`IMPROVED_SENSORS` for RCN) are not baked; workarounds are specified in Data fidelity. The missing distinctions are **NEEDS RULEBOOK / data** to author precisely.

---

## Cross-cutting notes

- **No bake change.** Every SBF number derives from `AsStats`, already baked verbatim from Mekbay. `crates/bake`, `data/mechs.bin`, and the bundle format are untouched (a filtered re-bake would clobber the ~10 MB `data/mechs.bin`). The only committed data added are the golden fixtures `data/sbf-goldens/*.json` + `data/sbf-goldens/input-parity.json` and the TUI `.snap` files.
- **Serialization / back-compat.** `GameMode::StrategicBattleForce` is an appended enum variant; the new `Session.sbf` field and every `SbfState`/`SbfFormationState`/`SbfUnitState` field carry `#[serde(default)]`, so sessions saved before SBF still load (`GameMode` defaults to `Classic`, `sbf` to empty). No `SESSION_VERSION` bump is needed — the AS-element pool already exists, so no `relink_specs`-style migration applies.
- **Cheat sheet + `?` modals.** Any new SBF keybinding is reflected in three places: the in-app AS/Override/SBF `?` modals in `view.rs`, `docs/keybindings-cheatsheet.html`, and the committed cheat-sheet PDF.
- **First-class GameMode.** SBF is a full `GameMode` with live tracking (damage/crits/turns plus a hand-set morale rung), never a read-only view.

---

## Code map

- `crates/core/src/engine/as_element.rs` — the typed AS element and SUA parser (§1).
- `crates/core/src/engine/sbf.rs` — the SBF converter (unit + formation, §2) and the combat submodule (§4).
- `crates/core/src/engine/mod.rs` — module registration for both.
- `crates/core/src/domain.rs` — the `GameMode::StrategicBattleForce` variant.
- `crates/core/src/session.rs` — `SbfState`/`SbfFormationState`/`SbfUnitState`/`MoraleStatus`, the `mech_cap`/`point_cost` arms, the derive + live-tracking + turn methods, and grouping ops.
- `crates/app/src/tui/app.rs` — `Screen::Sbf`, mode dispatch, the new-session modal, label matches.
- `crates/app/src/tui/view.rs` — the single-force SBF panes, the `t`/`c`/`m` editors, the `?` modal, label matches.
- `docs/keybindings-cheatsheet.html` (+ rendered PDF) — the SBF keybindings.
- `data/sbf-goldens/*.json` + `data/sbf-goldens/input-parity.json` — committed converter golden fixtures.
- `crates/app/src/tui/snapshots/neurohelmet__tui__tests__e2e_sbf_*.snap` — committed TUI snapshots.

---

## Appendix A — Morale simulation reference (NOT implemented)

*Preserved for a possible future auto-morale project. neurohelmet ships morale as a **manual rung** (§4.3); none of the below is wired up. Every number here is a MegaMek reading and several are contested (NEEDS RULEBOOK / IO:BF).*

**A.1 High-stress triggers** (would flag a formation as owing an end-of-round check; ACAR `StandardUnitAttackHandler`):
1. `hit_unit.current_armor * 2 < damage_just_applied` (one hit removed ≥ half remaining armor).
2. formation `is_crippled()` (§4.6) and the hit unit still has armor.
3. a unit was destroyed.
4. crit-roll 12.

**A.2 Morale-check TN** (`RecoveringNerveActionToHitData`): rulebook flat `3 + skill`; MegaMek adds a second skill term (net `2·skill − 2`, e.g. skill 4 → 7 not 6) — **contested**. Table (MegaMek net TN): skill 0→−2, 1→0, 2→2, 3→4, 4→6, 5→8, 6→10, 7→12, other→Impossible.

**A.3 Pass/fail** (`MoraleCheckActionHandler`): roll `2d6`; success iff `roll ≥ TN`. Failure worsens morale exactly one step (with the MegaMek `Unsteady` rung: `Normal→Shaken→Unsteady→Broken→Routed`; note neurohelmet's manual ladder drops `Unsteady`). Routed is terminal.

**A.4 Nerve recovery** (`RecoveringNerveActionHandler`): any formation with `morale > Normal` may attempt recovery at the **same TN**; the intended rule improves morale one step on **success** (`roll ≥ TN`). MegaMek's code improves on a roll *below* TN — an inversion bug.

**A.5 Morale → to-hit:** attacking a demoralized target gives the attacker a **−1/−2/−3** bonus for Shaken/Broken/Routed (IO:BF p.175 — demoralized = easier to hit). Distinct from the p.170 Engagement-Control roll modifier (+1/+2/+3), which is a different table and also unmodeled.

**A.6 Forced withdrawal** (`EndPhase.checkWithdrawingForces`): a `Routed` or crippled formation withdraws — **except BA/CI (infantry)**, which are exempt. In neurohelmet this survives only as a **non-triggering UI hint** (§4.3), never an automatic removal.




