# Standard BattleForce — Design & Reference

Standard BattleForce ("BF") is the hex-map, lance-scale game of *Interstellar Operations: BattleForce* (IO:BF pp.24–55 standard rules, pp.62–137 advanced) — the system Alpha Strike was derived from. Where SBF fuses a lance into one stat line, Standard BF keeps **per-element** stats and tracking; the lance ("Unit") exists only as a movement grouping. The book is explicit that the element data is the same data neurohelmet already bakes:

- *"use the Alpha Strike stats at masterunitlist.info, which are equivalent to BattleForce stats"* (p.53); *"Alpha Strike and BattleForce share the same unit conversions from Total Warfare"* (p.50).
- The only stat conversion in the whole system: *"Alpha Strike MV must to be converted from inches to BattleForce MV points. To do this, simply divide the MV value by 2"* (p.53, Panther 8″j → 4j) — exactly what `movement_hexes` (in `alpha_strike.rs`) already does for the AS ground-scale toggle.
- The printed BF ground record sheet (p.322–324) is, field for field, an AS card grouped under a Unit wrapper: per element `MV · S(+0)/M(+2)/L(+4)/E(+6) · SZ · Skill · OV · Armor/Structure bubbles · Heat Scale 1 2 3 S · Special Abilities · Destroyed☐`; per Unit `Unit Name / Unit MV / Size / Point Value / Notes`.

There is nothing to convert and no MegaMek port: MegaMek has no interactive Standard BF engine, and none is needed. neurohelmet owns the rules and IO:BF is the sole authority — the same footing as SBF's rules-owned half. All tables below are transcribed from the book with page cites; the back-of-book quick-reference tables (pp.344–345) serve as an independent second printing to cross-check every table, and disagreements are flagged.

Where Standard BF sits among the modes: Classic → Alpha Strike → **Standard BF** → SBF → ACS. Roughly the whole mode is the existing AlphaStrike tracking core; the deltas are the type-columned crit table, the BF to-hit table, hex-native range labels, and the lance grouping with a live "Unit MV = slowest survivor" readout.

**Methodological invariant:** where the *standard-rules* chapter and the *advanced-rules* chapter print different numbers for the same thing (it happens — physical-attack to-hit rows, grounded-DropShip modifier), the mode implements **one** table; each such conflict is resolved explicitly in the Rulebook-conflicts section, not silently. Where BF and Alpha Strike:CE disagree (attacker standstill −1, no to-hit floor, E = L−1), **BF wins inside this mode** — AlphaStrike mode is untouched.

---

## Page citations

All `p.NN` citations reference *Interstellar Operations: BattleForce* (IO:BF). Chapter map: Standard Rules pp.24–55, Warfare Symbology p.56, Advanced Rules pp.62–137, Special Abilities pp.142–157, SBF from p.162, record sheets and quick-reference tables pp.322–354.

---

## Scope — single force, boardless, manual-first

The SBF spec's Scope section applies wholesale: neurohelmet tracks **one force — yours**; two-sided tracking, initiative procedure, and turn interleave are **non-goals**; the only cross-side data is the target's numbers, hand-entered at to-hit time. Standard BF adds two mode-specific scope rulings:

1. **Damage timing — apply immediately.** The book defers all Combat-Phase damage, crit effects and destruction to the End Phase (*"all damage inflicted during the Combat Phase takes effect during the End Phase"*, p.49 [p.65 advanced]; a destroyed element still shoots back, and attacks on an already-dead target auto-hit, p.37). That simultaneity is a **table procedure**: the players know what is pending; the app records outcomes as they are announced. neurohelmet applies damage the moment it is entered, exactly like AlphaStrike/Override/SBF mode. This immediate application is a documented non-goal, not an attempt to model the book's End-Phase simultaneity.
2. **The board stays at the table.** Movement costs, terrain, LOS, firing arcs, facing, stacking, minefields, artillery scatter, aerospace altitude/velocity bookkeeping — all TABLE-CONCERN. The app tracks record-sheet state (armor/structure/heat/crits/morale/grouping) and computes numbers (to-hit, live MV/TMM, Unit MV, damage readouts). Terrain and attack-flavor to-hit rows appear in the calculator as hand-set toggles, exactly like SBF's `SbfToHitCtx.terrain`.

**In scope** (the deltas over AlphaStrike mode): the BF to-hit table (p.39) with live attacker-side derivation; live MV/TMM from heat + crits via the Available-MP bracket table (p.347 quick-ref; basis fn1 p.86); the type-columned crit table (p.42 = p.87) with BF effect semantics replacing `AsCrits`; the motive-damage table (p.44); physical-attack TN/damage readouts (pp.43–45); OV commitment arithmetic incl. OVL (p.49, p.151); Unit (lance) grouping with live Unit MV/jump and static Unit Size (pp.52–53); manual per-Unit morale rungs Normal/Broken/Routed (pp.97–99, manual per the SBF morale ruling); air-to-ground attack readouts for your own fighters (bombing/strafing/striking, pp.47–48); aerospace elements tracked like AS mode does (TH threshold crit trigger, aero crit column, aero range labels).

**Out of scope for v1** (each is a known non-goal): artillery resolution (ART specials still render; the artillery to-hit rows are Advanced-chapter material — limitation 8), alternate munitions, DropShip/large-craft columns beyond display (not in the baked catalog), squadrons, commands/CP economy, battlefield intelligence, hidden units, environmental conditions, the generic-CI trooper-scaling tables (printed on the CI record sheet, p.26 — neurohelmet's catalog bakes real CI elements with their own stat lines; limitation 9), and transport/mounted state (mounted passengers, XMEC/MCS carrier penalties — the p.52 Unit-MP rule says "dismounted surviving Elements"; untracked).

---

## Reference sources

**Rulebook (the only rules authority):** *Interstellar Operations: BattleForce*. Load-bearing pages: 26 (record sheet fields), 25 (rounding, terms), 27–31 (scale note, movement basics, Unit MP), 37–49 (combat phase end to end), 42 (crit table + effects), 44 (motive), 49 (heat/shutdown/end phase), 50–55 (skill PV, force org, units & formations, unit size), 84–89 (advanced combat: ground Extreme, expanded crit table p.87, artillery), 97–99 (morale), 142–157 (special abilities), 322–324 (ground record sheets), 344–345 & 347/350 (quick-reference second printings — the TMM bracket table survives only at p.347).

**neurohelmet code map:**
- `crates/core/src/domain.rs` — `GameMode` (BF variant).
- `crates/core/src/session.rs` — `TrackedMech` + AS live fields, `AsCrits`, `AsTarget`, AS tracking methods, `point_cost`, `skill_adjusted_pv` (`engine/skill.rs`), `mech_cap`, the `remove_mech` index-remap precedent, the `Session.sbf` field pattern, SBF grouping ops.
- `crates/core/src/engine/alpha_strike.rs` — `inches_to_hexes`, `movement_hexes`, `as_to_hit_full` (for contrast; **not** reused for BF math — brackets and modifiers differ).
- `crates/core/src/engine/as_element.rs` — `as_element`, `AsElement`, SUA queries, `DamageVector`, `jround`. The SBF-era typed parser is the access layer this mode reads specials/movement through.
- `crates/core/src/engine/sbf.rs` — `SbfRange`, `DamageVector::band`, `reduced_by` as shape precedents; `SbfToHitCtx`/`sbf_to_hit` as the ctx-struct pattern.
- `crates/app/src/tui/{app.rs,view.rs}` — the full GameMode wiring surface (see the TUI section). `forcegen.rs`, `export.rs`.
- Docs precedents: `docs/sbf-implementation-spec.md`, `docs/sbf-smoke-test.md`, `docs/keybindings-cheatsheet.html` (and the rendered PDF).

---

## Data fidelity & gaps

The mode's input is the existing baked `AsStats` + `TrackedMech.gunnery`, read through `as_element()`. Confirmations and gaps, measured against the rulebook:

1. **Stats transfer unchanged** (p.50, p.53): S/M/L damage values, Armor, Structure, Size, Skill, OV, TH, PV, specials are the AS card values. Only MV converts (÷2). `movement_hexes` already renders it; it uses `div_ceil(2)` while the book says "divide by 2" with an even-inch example — no catalog element has an odd-inch MV, so the two never diverge (limitation 6).
2. **TMM is derived, not printed.** The BF ground sheet has **no TMM box**; the modifier comes from the Available-MP bracket table (numeric brackets survive extraction only in the p.347 quick-ref; the basis is p.86 fn1: *"based on available MP modified by heat level and critical hits … MP expended are irrelevant. Does not apply to aerospace Elements."*). The bracket table is numerically identical to the AS TMM table at 2″=1 hex, so at full health `bf_tmm(current_mp)` equals the baked `AsStats.tmm`. A catalog sweep confirms this holds for every ground element; the rule wins live (limitation 5).
3. **Extreme range**: the record sheet prints an E column for ground elements but the standard ground range table has no E bracket (table p.38; *"Ground Elements have 3 ranges"* p.39; *"only aerospace Elements use extreme range"* p.41; quick-ref p.344); ground Extreme is an Advanced rule — 9–10 hexes, **damage = Long value − 1, min 0, computed at attack time** (p.84) — NOT the baked `dmg_e` (which is aerospace-only data). `bf_damage(band)` special-cases ground-E.
4. **Range brackets are hex-native and NOT AS-inches÷2** (p.38/p.344): ground S 0–1 / M 2–4 / L 5–8 (E 9–10 advanced); underwater S 0 / M 1–2 / L 3–4 (all underwater ranges halved, p.39 fn2); air-to-air S 0–32 / M 33–64 / L 65–107 / E 108–133. The damage *values* are AS; only the bracket geometry differs (BF hexes are 90 m, p.27). AS mode's `range_brackets_hexes()` labels are wrong for this mode — BF gets its own label strings.
5. **Specials are the same catalog, hex-unit text** (pp.142–157). The parser already handles every token. Only these feed the calculator: `AFC`/`BFC` (support-vehicle fire-control rows), `SHLD` (+1 own attacks, weapons only), `STL` (bracket-dependent target rows; weapon attacks only, p.151), `MAS`/`LMAS` (+3/+2 vs a target that stood still **or is immobile**; weapon attacks only — p.148 "immobile or remained at a standstill … weapon attacks (but not physical attacks)"), `LG/SLG/VLG` (target −1), `JMPS#/JMPW#` / `SUBS#/SUBW#` (±# target jump/submersible TMM), `MEL` (+1 physical damage), `TSM/I-TSM/TSMX` (heat-conditional +1 MP and physical +1 damage; I-TSM +2 TN on physicals — §1.2/§1.5), `OVL` (OV extends to L), `IF#` (indirect damage = #), `REAR#` (rear-weapons attack: +1 TN, REAR damage values, forward reduction — §1.6), `FLK#` (miss-by-≤2 damage vs airborne + the −2 ground-to-air row, p.85 fn6), `CASE/CASEII/CASEP/ENE` (ammo-crit outcome), `CR/ARM/IRA/RFA` (crit-roll modifiers on your own element — §1.4), `EE/FC` (engine-crit variant — §1.4), `BHJ` (hull-breach immunity), `ENG/SAW` (charge rounding), `BOMB#` (each carried bomb −1 TP). Everything else renders as reference text exactly like AS mode. Known book-side quirks that do NOT block this mode (flagged for completeness): LECM printed with the same 1-hex bubble as ECM (editing error), BH range given in inches, iATM Magnetic Pulse sign suspect.
6. **CI elements carry two skills in BF** (Gunnery + Anti-'Mech, p.26). neurohelmet tracks one AS skill (`gunnery`). v1 uses `gunnery` for both and the physical calculator's anti-'Mech row supplies the type offset (limitation 9).
7. **Not baked, columns unreachable:** ProtoMechs (PM) and DropShips/large craft (DS/DA/JS/WS/SS) are absent from the catalog (same gap list as the SBF spec). The mode implements the ProtoMech and DropShip crit columns anyway (cheap, closes the table), but they are dead code today; the JumpShip column (p.87 only) is **omitted** — it is unreachable and standard play never uses it.
8. **BD (neurohelmet-local gun emplacements)**: treated as the Vehicle crit column, immobile (TMM −4 basis), weapons-only crit vocabulary as in AS mode (`AsCritKind::WEAPON_ONLY` precedent). The BD crit modal suppresses the motive rows entirely; the non-Weapon 2D6 rows render dimmed with "(+1 damage instead, p.42)", and applying one applies 1 damage — the p.42 doesn't-apply rule — not the effect.

---

## The rules module (`engine/battleforce.rs`)

The pure rules layer is `crates/core/src/engine/battleforce.rs`, registered in `engine/mod.rs`. It depends on `as_element.rs` (typed element) and reuses `sbf::SbfRange` as the range-bracket enum (the four-band semantics are identical, so `pub use sbf::SbfRange as BfRange` or plain reuse both work).

All arithmetic uses the book's named rounding (p.25): *round up / round down / round normally* (round normally = half-up, i.e. `jround`). Each function below names its mode.

### 1.1 Range table & labels (p.38, second printing p.344)

```rust
/// Ground: S 0-1, M 2-4, L 5-8, E 9-10 (E is the Advanced ground bracket, p.84).
/// Underwater: S 0, M 1-2, L 3-4 (and all underwater ranges are halved, p.39 fn2).
/// Air-to-air: S 0-32, M 33-64, L 65-107, E 108-133.
pub fn bf_range_label(aero: bool) -> &'static str; // "S 0-1  M 2-4  L 5-8  E 9-10*" / air-to-air string
```

Damage at bracket (p.39, p.41, p.84): S/M/L = the element's card values; **ground E = max(L − 1, 0)** computed (never the baked `dmg_e`); aerospace E = baked `dmg_e`. A dash/0 bracket = no attack at that range; `0*` = minimal damage (on-hit 1D6: 3+ → 1 damage, else 0 — p.41; render as "0*" and let the table roll it, like AS mode does).

```rust
pub fn bf_damage(el: &AsElement, range: BfRange, weapon_crits: u8) -> Option<f32>;
// card band − 1 per Weapon crit, floored 0 (p.43); ground E derived as above; None = can't attack.
```

### 1.2 Live MV, TMM, and the crit/heat arithmetic they depend on

```rust
/// Current available ground MP: base hexes (+TSM rule) − heat level − accumulated MP-crit loss,
/// then the motive-damage flags (−1 if minus_one; × 0.5 round down if half; 0 if immobile) and the
/// live Engine-crit effect, floored 0. Heat subtracts from MP directly (p.49). Aerospace: same for
/// TP (heat likewise). mp_lost comes off BEFORE the motive/engine halvings, matching chronological
/// table play.
pub fn bf_current_mp(base_hexes: u32, heat: u8, mp_lost: u32, motive: BfMotive, tsm: bool,
                     engine_hits: u8, col: Option<BfCritCol>) -> u32;
// tsm carries the TSM rule; engine_hits + col carry the §1.4 Engine-crit MV/TP effects, derived
// live — vehicle col & ≥1 hit: MV × 0.5 round down; aero col & 1 hit: thrust −50% of current,
// round down, min 1 lost; aero col & 2 hits: TP 0. Nothing engine-related is ever snapshotted into
// mp_lost, which holds MP crits only.

/// TMM from available MP (numeric brackets: p.347 quick-ref; basis: p.86 fn1; identical to the AS
/// TMM table at hex scale): 0-2 → +0, 3-4 → +1, 5-6 → +2, 7-9 → +3, 10-17 → +4, 18+ → +5.
/// Ground only (fn1: "Does not apply to aerospace Elements").
pub fn bf_tmm(available_mp: u32) -> i32;
```

**TSM interaction** (p.154): a TSM element at heat ≥ 1 gains **+1 MP**, and at heat 1 *ignores* the 1-MP heat loss entirely (heat 2+ subtracts normally); the movement effect is **not pre-baked into AS stats**. `bf_current_mp` takes the element (or a `tsm: bool`) and applies this before the TMM bracket — otherwise every TSM 'Mech under heat shows a wrong live MV/TMM. I-TSM/TSMX have no movement effect (already in stats).

**MP crit** (p.43): each hit removes **half of CURRENT MP, rounded normally, minimum 1 lost** — multiplicative at apply time, so it is not `count × k`. The state layer stores the accumulated `mp_lost` and computes the loss when the crit is applied (`loss = max(1, jround(current as f64 / 2.0))`). At 0 MP the element cannot move (aero at 0 TP: velocity frozen — table concern beyond the badge).

**Motive damage** (vehicles, p.44): `BfMotive { minus_one: bool, half: bool, immobile: bool }` — independent once-per-game **spent-flags**. *"A vehicle may only suffer each effect once per game"* (p.43) limits repeats of the SAME effect, not combinations — a vehicle that rolls 8–9 then 10–11 has both −1 MV **and** MV×0.5 (even MV differs: MV 8 → (8−1)/2 = 3, not 4). Flags only ever set (`bf_mark_motive`); re-marking is a no-op. The chance roll (1D6 1–4 no effect, 5–6 effect) and the effect roll (2D6: 2–7 none, 8–9 −1 MV, 10–11 ½ MV, 12 immobilized; modifiers Tracked/Naval +0, Wheeled +2, Hover/Hydrofoil +3, VTOL/WiGE +4, rear hit +1) stay at the table; the app records the flags. Crash rider (p.43): a VTOL/WiGE reduced to 0 MV while ≥1 elevation up crashes — 1 damage (crit-check if it damages structure) + immobile; the modal surfaces both.

```rust
pub fn bf_motive_effect(roll_2d6: i32) -> BfMotive; // table reader for the crit modal's dim reference
```

### 1.3 The to-hit calculator (p.39; second printings p.345 and p.86)

*"The Base To-Hit number for all attacks is the attacking Element's Skill Rating"* (p.39); LOS/range are Unit-to-Unit but *"the to-hit number is calculated for each Element individually"*. No floor — BF states none (AS:CE's floor-2 is an AS rule; divergence noted, BF wins here).

```rust
pub struct BfShot {
    pub range: BfRange,
    pub kind: BfAttackKind,       // Standard | Indirect { spotter_also_attacked: bool, spotter_is_remote_sensor: bool }
                                  //   | RearWeapons | Physical(BfPhysical) | AirToGround(BfA2G)
    pub attacker_move: BfMove,    // StoodStill | Moved | Jumped
    pub area_effect: bool, pub secondary: bool, pub also_spotting: bool,
    // target side — hand-entered, mirrors AsTarget/OvShot: target_move:
    // BfTargetMove { Standstill | Ground | Jumping | Submersible | Dropped } + target_move_adj: i32
    // for the ±JMPS/JMPW/SUBS/SUBW rows; plus grounded, target_mas/_lmas, target_carrying_ba, and
    // AirborneAero(BfAeroAngle) for the 3-way:
    pub target_tmm: u8, pub target_immobile: bool,
    pub target_kind: BfTargetKind, // None | BattleArmor | ProtoMech | AirborneAero | AirborneDropship | AirborneVtolWige | Large
    pub target_woods: bool, pub target_partial_cover: bool, pub target_underwater: bool, pub target_stealth: bool,
}
pub enum BfPhysical { Standard, Melee, Charge, Dfa, AntiMech }
pub enum BfA2G { AltitudeBombing, DiveBombing, Strafing, Striking }
pub fn bf_to_hit(el: &AsElement, skill: u8, heat: u8, fc_crits: u8, shot: &BfShot) -> i32;
```

**Physical eligibility by type** (p.43–45; the modal greys ineligible picks): 'Mechs may use Standard, Melee and Special (Charge/DFA); ProtoMechs **Standard only**; vehicles **Charge only**; MEL elements **may not choose Standard instead** (p.44); DFA requires jump capability and may not target airborne aerospace (p.45 — `bf_shot_for` sanitizes a DFA-vs-airborne-aero declaration back to Standard and the shot modal shows the warning line); Anti-'Mech requires an infantry element with the AM special — BA innately, CI must bake `AM` (p.143).

Modifier rows, exactly (p.39 + footnotes; ✻ = derived from the element, not hand-entered):

| Group | Row | Mod |
|---|---|---|
| Attacker move | Standstill | −1 (not infantry/BA, fn1) |
| | Ground/minimum | +0 |
| | Jumping | +2 (not infantry/BA, fn1) |
| Attacker state ✻ | Heat level | +heat (weapon attacks only, fn8) |
| | Fire Control crit | +2 each (weapon attacks only, fn7) |
| | SV, neither AFC nor BFC | +2 |
| | SV with BFC | +1 |
| | IndustrialMech without AFC | +1 |
| | SHLD | +1 (weapon attacks only, fn6) |
| | Grounded aerospace fighter/CF | +2 on ground-to-ground weapon attacks (p.46; hand-toggle) |
| Attack | Area-effect | +1 |
| | Indirect fire | +1 (+2 if spotter also attacked; remote-sensor spotter: additional +3 — fn4) |
| | Secondary target | +1 |
| | Attacker is also spotting | +1 |
| | Using REAR special ability | +1 (rear-mounted weapons, §1.6 — *not* "attack strikes target's rear", which is +1 damage, p.41) |
| | Altitude Bombing / Dive Bombing / Strafing / Striking | +3 / +2 / +2 / +2 (p.85; bombing excludes immobile & target-hex terrain mods, p.47) |
| Range | S / M / L / E | +0 / +2 / +4 / +6 |
| Target move | Standstill/minimum | +0 |
| | Ground | +TMM |
| | Jumping | +TMM+1 (±JMPS/JMPW #) |
| | Submersible | +TMM+1 (±SUBS/SUBW # — the standard printing's "JMPS#" labels here are typos; the p.86 copy corrects them) |
| | Dropped by airborne Unit | +3 |
| | Immobile | −4 (overrides TMM; shutdown targets are −4 with no TMM — p.39 table, p.49, p.86 fn13) |
| Target type | Battle Armor | +1 |
| | ProtoMech | +1 |
| | Large (LG/SLG/VLG) | −1 |
| | Airborne aerospace | +2 (angle of attack: nose +1 / sides +2 / aft +0, p.86 fn10; the +2 row is the side default — expose the 3-way in the modal) |
| | Airborne DropShip | −2 |
| | Airborne VTOL/WiGE | +1 |
| | STL active | +0/+1/+2 by S/M/L (BA targets +1/+1/+2), fn11 |
| Terrain | Woods / Partial cover / Underwater | +1 / +1 / +1 (underwater only if attacker also submerged, fn2) |
| Physical | Charge / DFA / Anti-'Mech / conv.-infantry attacker / target carrying BA | +1 / +1 / +1 / +3 / +3 |

Physical attacks exclude heat, FC crits, and SHLD (fns 6–8); the target-side STL and MAS/LMAS rows are likewise weapon-attacks-only (pp.148/151). The Anti-'Mech reading that reconciles standard and advanced printings: BA anti-'Mech = +1; conventional infantry = +1 anti-'Mech **+3 conv-infantry attacker** = +4 total, matching the advanced table's flat "+4" (p.85) and the specialty-infantry text "Skill +4 conventional / +1 battle armor" (p.124). The ground-to-air **Flak −2** row (p.85, fn6 on p.86: *"ground-to-air attacks against airborne aerospace, VTOL and WiGE targets only"*) applies when the attacker has FLK, the target is airborne aero/VTOL/WiGE, and the attack is a **Standard weapon attack** made **ground-to-air** — derived, not hand-entered: a non-aero attacker is always ground-based; an aero attacker only with the grounded toggle on (p.46). REAR attacks never use flak (p.152 "REAR attacks cannot make use of other special attack abilities, such as heat, indirect fire, flak, or artillery"). The modal notes the miss-by-≤2 FLK damage consolation (p.148) and says when the −2 is not folded in.

Two printed conflicts are resolved here (details in the Rulebook-conflicts section): **grounded-DropShip attacker = −2** (prose p.46 + advanced table p.85 over the standard table's −1); **physical rows use the standard chapter's +1/+1** (the advanced table prints Charge +2 / DFA +3 on p.85 — limitation 2).

### 1.4 The crit table (p.42; expanded p.87; second printings pp.345/350)

The standard table (p.42) and the "Expanded" table (p.87) are **the same table** for every column neurohelmet can field — p.87 adds only a JumpShips column and broader footnotes. One implementation:

```rust
pub enum BfCritCol { Mech, ProtoMech, Vehicle, Aerospace, DropShip }
// from element: BM/IM → Mech (IndustrialMechs roll TWICE, apply both — p.42);
// PM → ProtoMech; CV, ground SV, BD → Vehicle; AF/CF, fixed-wing SV → Aerospace;
// SC + the DS family → DropShip (both printings footnote the DropShips column "Includes Small
// Craft" — p.42 ‡ / p.87 ‡ — and p.43 gives DS/SC the 3-stage engine ladder. Mark-only until
// small craft are baked).
pub enum BfCrit { NoCrit, Ammo, Engine, FireControl, Mp, Weapon, CrewStunned, CrewKilled,
                  Fuel, HeadBlownOff, ProtoDestroyed, KfBoom, DockingCollar, Thruster, Door, CrewHit }
pub fn bf_crit(roll_2d6: i32, col: BfCritCol) -> BfCrit;
```

| 2D6 | 'Mech | ProtoMech | Vehicle | Aerospace | DropShip |
|---|---|---|---|---|---|
| 2 | Ammo | Weapon | Ammo | Fuel | KF Boom |
| 3 | Engine | Weapon | Crew Stunned | Fire Control | Docking Collar |
| 4 | Fire Control | Fire Control | Fire Control | Engine | No Crit |
| 5 | No Crit | MP | Fire Control | Weapon | Fire Control |
| 6 | Weapon | No Crit | No Crit | No Crit | Weapon |
| 7 | MP | MP | No Crit | No Crit | Thruster |
| 8 | Weapon | No Crit | No Crit | No Crit | Weapon |
| 9 | No Crit | MP | Weapon | Weapon | Door |
| 10 | Fire Control | Proto Destroyed | Weapon | Engine | No Crit |
| 11 | Engine | Weapon | Crew Killed | Fire Control | Engine |
| 12 | Head Blown Off | Weapon | Engine | Crew Killed | Crew Hit |

**Trigger conditions** (p.42; the player rolls, the app tells them a roll is owed — mirror SBF's `crit_check_due` decomposition): any hit that damages structure; any damage to a BAR element (p.143: *"always trigger a roll … regardless"*); aerospace: single-attack damage > TH threshold; **hull breach** — every hit on a fully-underwater element owes a crit chance regardless of structure damage (partially submerged negates on 2D6 ≥ Skill+2; BHJ-family elements are immune — p.41, p.144; underwater damage itself is ×0.5 round down min 1, ENE attackers full, TOR added after at full value). Infantry and BA never take crits.

**Crit-roll modifiers on the defender** (dim reference in the modal, since the roll is against *your* element): CR −2, modified ≤1 = No Crit (p.145); IRA +1, modified >12 = Engine Hit (p.148); RFA +2, modified 13+ = Engine Hit (p.152); ARM ignores the **first** crit chance of the scenario — a spent-checkbox on the sheet, so `BfLive` carries `arm_spent: bool` (p.143). *"The effects of Critical Hits are permanent"* (p.42). A result that doesn't apply to the unit, or a once-per-element crit rolled again → **+1 damage instead, no chained crit roll** (p.42).

**Effect semantics** (pp.42–43) — what `apply_crit` does per result:

- **Ammo**: destroyed, unless CASE (**takes 1 damage**, crit-check that damage normally), CASEII/ENE (**ignore**), or CASEP (1D6: 3+ ignore, ≤2 destroyed — p.151; the modal prompts for the roll). Otherwise the modal reads the element's specials and offers the single correct outcome.
- **Engine ('Mech)**: 1st = +1 heat every turn it fires weapons (a badge; heat is manual) — 2nd = **destroyed** (matches `as_destroyed`'s 2-engine rule). **EE/FC elements** (non-fusion — many IMs/SVs) take no heat effect; their badge reads "after engine hit: 2D6 each End Phase it fired — 12 = explodes" (p.146).
- **Engine (Vehicle)**: 1st = MV ×0.5 **and** all damage values ×0.5 (round down, min 0 — p.43); 2nd = destroyed. Both effects derive **live** from the persisted hit count — MV in `bf_current_mp`, damage in the `bf_shot_damage`/`bf_indirect_damage`/`bf_rear_damage` shared halving leg, which `Session::bf_current_damage` and every modal preview route through — never snapshotted into `mp_lost`.
- **Engine (Aerospace)**: 1st = thrust −50% (round down, min 1 lost); 2nd = TP 0 + shutdown. Likewise live-derived from the hit count in `bf_current_mp`, so cooling heat after the crit cannot resurrect thrust; `mp_lost` holds MP crits only.
- **Fire Control**: +2 to-hit per hit, cumulative, never on physicals (p.43).
- **MP**: −50% of current MP/TP, round normally, min 1 lost (§1.2).
- **Weapon**: all damage values −1 (min 0), including the damage-bearing specials AC/ARTx/FLK/HT/IF/LRM/SDS/SRM/TOR/TUR (p.43).
- **Crew Stunned (Vehicle)**: no attacks **next turn** (a turn flag, cleared manually/`n`).
- **Crew Killed / Fuel / Head Blown Off / Proto Destroyed**: destroyed.
- DropShip column results: implemented as marks with the book text in the modal; unreachable in the baked catalog (Data fidelity 7).

### 1.5 Physical attacks (pp.43–45) — TN rows in §1.3, damage readouts here

```rust
pub fn bf_physical_damage(kind: BfPhysical, el: &AsElement, available_mp: u32, heat: u8) -> f32; // heat gates the TSM +1
```

- **Standard / Melee**: attacker Size; MEL +1 (p.44); TSM (at heat ≥1) / TSMX / I-TSM each +1 (I-TSM also +2 TN on physicals — pp.149/151/154).
- **Charge**: available MV × size multiplier (Size 1/2/3/4 → ×0.25/0.50/0.75/1.0), round normally; ENG/SAW vehicles round up. Attacker takes target-Size damage on success (vehicle attackers also roll motive).
- **DFA**: charge damage + 1; requires jump; attacker takes own Size (Size+1 on a miss); one crit roll vs target regardless of structure damage + one more if structure damaged (p.45).
- **Anti-'Mech** (infantry): normal damage + one crit roll on success (p.124).
Physical damage is never overheat-boosted (p.49); Weapon crits don't reduce it (p.43).

**Air-to-ground damage readouts** (pp.47–48; your own fighters — the board geometry of flight paths/scatter stays at the table, the numbers don't):
- **Strafing**: half the S value, round normally, min 1 — overheat commit and the rear +1 are added **before** halving; hits every element in the strafed hexes.
- **Striking**: S value + overheat, +1 if rear.
- The rear +1 is a shot-modal **"Strikes rear (+1 dmg)" toggle row**, active for Strafing/Striking only, threaded into both previews — damage-side only, no TN row.
- **Bombing** (HE/Cluster): 2 damage per bomb to all elements in the hex; Inferno: +2 heat to 'Mechs/landed fighters (non-stacking), 2 damage to PM/BA, destroys non-BA infantry, no effect on DropShips; *"Bombing attacks never strike a Unit from the rear"* (p.48). **Altitude bombing drops exactly one bomb per hex** — *"attack one hex for each bomb … must drop one bomb in each hex"* (p.47) — so its preview prices the flat per-hex 2 plus the hex count; only Dive Bombing previews the all-bombs-in-one-hex aggregate. Bombing/strafing/striking always resolve at **Short** range (p.47). Each carried bomb is −1 TP (p.30) — a BOMB-carrier badge.

### 1.6 Overheat commitment (pp.48–49; OVL p.151)

Committed at declaration: 1..=min(OV, heat-room) extra damage added *"at all range brackets for which it has a damage value"* — **S/M only unless OVL** (p.151: elements without OVL apply overheat at Short/Medium only); never on REAR (p.152), IF, or physicals (p.49).

**REAR-weapons attacks** (pp.151–152): a `RearWeapons` shot uses the element's **REAR#/#/#(/#) values, not the card S/M/L**, at +1 TN; firing REAR and forward in the same turn reduces the forward damage 1-for-1 per point of REAR damage dealt, applied **before** overheat — the shot modal composes and shows the reduced forward line whenever a REAR shot is declared: `fwd after REAR: max(0, current_damage(range) − REAR dealt) [+OV where bf_ov_applies]`. Distinct from *being hit* in the rear, which is +1 damage and no TN row (p.41). Heat gained = amount used (−1 if in water). Voluntary +1 heat allowed regardless of OV. HT-inflicted heat caps at +2/turn (p.49). Heat effects (MP −heat, TN +heat) bite next turn per the book; in-app they bite when the player marks the heat (apply-immediately ruling, Scope 1). Cooldown is manual (`o`/`i` adjust, the AS-mode heat keys in `app.rs`) — the BF auto-cooldown ("heat → 0 in any End Phase without a weapon attack", shutdown auto-restart at heat 0 after one turn, p.49) is a table procedure the player executes; the help modal documents it and the app does not automate it (manual-first).

```rust
pub fn bf_shot_damage(el: &AsElement, range: BfRange, weapon_crits: u8, ov_commit: u8,
                      engine_hits: u8) -> Option<f32>;
// engine_hits — also on bf_indirect_damage / bf_rear_damage — carries the vehicle Engine-crit
// halving, applied after the Weapon-crit subtraction and before the OV add; the crit column is
// derived internally via bf_crit_col. Session::bf_current_damage is this with a zero commit, so
// the card and the shot-modal previews share one damage path.
```

### 1.7 Unit (lance) derived stats (pp.52–53)

```rust
pub fn bf_unit_mv(members: &[(u32 /*current ground mp*/, Option<u32> /*current jump*/, bool /*alive*/)]) -> (u32, Option<u32>);
pub fn bf_unit_size(sizes: &[u8]) -> i64; // jround(mean) — static at grouping time, never recomputed (p.53)
```

- *"A Unit's MP always equals the lowest MP of any of its dismounted surviving Elements"* (p.52; the p.28 printing lacks "dismounted" — mounted-passenger exclusion is part of the untracked transport state, Scope 2); per **mode**: ground minimum and jump minimum computed separately (p.52's 5-MP-ground / 3-MP-jump example).
- *"The Unit is considered jump-capable (j) only if all surviving Elements … have Jumping MP"* (p.52); recompute on every heat change, MP/engine/motive crit, destruction, or membership change (p.52: *"Players must recalculate a Unit's MP during play"* — this recalculation is exactly what the mode automates).
- Unit Size: sum ÷ count, round normally; *"determined at the start of play, and is not adjusted for destroyed Elements"* (p.53) — stored at grouping time, invalidated only by membership edits.
- A shutdown element pins the Unit (*"Ground Units containing a shutdown Element cannot move; however, the other Elements' MP ratings are unaffected"*, p.49) — a Unit badge, not an MP change.
- Legal Unit sizes: the printed **ground** sheets hold 4 (IS) / 5 (Clan) / 6 (CS/WoB) element slots (pp.322–324); the **aerospace** sheet holds 2 per Unit (Air Lance / Point, p.325; Force Distribution p.51 confirms IS 2 / Clan 2 / CS 2 for aero) — the doctrine auto-group uses 2 for aero units, not the ground sizes. Understrength allowed; advanced hard cap **9 elements per Unit** (p.102) — enforced as a non-blocking warning (SBF `can_convert` stance), noting the book's own Clan vehicle/aero Stars are 10 elements (p.51), so the warning firing on a legal Clan Star is expected, not a bug. DropShips/Large SEs never join Units (p.51, p.53) — unreachable today, note only.

### 1.8 Skill-adjusted PV (p.50)

The BF Skill PV table **is** the AS one: decrease brackets 0–14→1 … 95–104→10 (+1 per 10 over) per skill step above 4; increase brackets 0–7→1 … 48–52→10 (+1 per 5 over) per step below 4; bracket looked up **once from base PV** (p.52 example), floor 1. The mode reuses `skill_adjusted_pv` (`engine/skill.rs`) unchanged; the BF brackets are identical to AS.

### Worked examples

Hand-computed against the book's own examples:
- To-hit (p.40): Skill 3 + medium → 5; Skill 3 + long + TMM 2 + LG target → 8; Skill 3 + short + TMM 2 + target-jumped + target-in-water terrain +1 → 7. Physical attacks exclude heat/FC/SHLD and the target-side STL/MAS/LMAS rows (weapon-only, pp.148/151); MAS applies on immobile targets; FLK ground-to-air gating excludes air-to-air/REAR/airborne-aero attackers and includes grounded aero; A2G kinds add +3/+2/+2/+2, bombing excluding the immobile and target-hex terrain rows.
- Strafing damage: S 5 + OV 2 committed → (5+2)/2 = 3.5 → 4 (round normally, overheat added before halving); S 1 → 1 (min 1). Striking: S + OV.
- `bf_tmm` bracket edges: 2→0, 3→1, 5→2, 7→3, 10→4, 18→5.
- `bf_crit`: the full table (§1.4); IndustrialMechs roll twice and apply both.
- MP crit sequence (p.43): MV 8, heat 0 → first crit loses 4 (8/2); second loses 2; at current 1, loss floors at 1 → 0 = immobile.
- Motive flags: stacking (base 8, −1 and ½ → 3); live order (base 6, mp_lost 3, ½ → 1).
- Vehicle engine crit: live MV halving (MV 6 → 3, independent of motive ½); live damage halving on every readout (S/M/L/E, IF, REAR — after Weapon crits, before OV); second hit → destroyed. Aero engine: live first-hit −50%-of-current and second-hit TP 0 that survives heat repair.
- Charge (p.44): Size 2, MV 6 → 3; Size 3, MV 5 → 3.75 → 4 (round normally); DFA = charge + 1.
- Overheat: OV 3, dmg 4/4/− → 7/7/− max; OVL extends to L; heat-room caps commit at heat 2 → max 2.
- `bf_damage` ground E: L 2 → E 1; L 0* → E 0 (no attack); aero E = baked.
- Unit stats (p.52): 3 jumpers (3j) + 2 walkers (2) → (2, None); all-jump unit → (min, Some(min j)); `bf_unit_size`: [1,2,1,2] → 2, [4,3,3,3,3,4] → 3.
- Skill-adjusted PV (p.52): base 35 skill 6 → 27; base 39 skill 2 → 55; base 30 skill 2 → 42 (Alice example).

---

## Session state & live tracking

### 2.1 GameMode variant

`GameMode` (`domain.rs`) carries a `BattleForce` variant — doc comment: "Standard BattleForce (IO:BF pp.24–55): per-element AS-card tracking at hex scale, lance Units, BF crit table. Single-force; uses AS PV. See docs/standard-bf-implementation-spec.md". Mode matches: `point_cost` → `skill_adjusted_pv` arm (with AS/SBF); `mech_cap` → `None`; `new_with_mode` seeds one empty `BfUnitState { name: "Unit 1" }` (the SBF starter-formation precedent). Serde: appended variant + `#[serde(default)]` everywhere = old sessions load untouched.

### 2.2 Per-element live state — reuse the AS fields, add BF crits

The AS block on `TrackedMech` is reused as-is: `as_armor_hits`, `as_struct_hits`, `as_heat` (same 0–4/S scale, p.26); `as_attacker_jumped` is superseded by the shot modal's 3-way move selector (BF-only; AS mode untouched). **`AsCrits` is not reused** — BF's vocabulary and arithmetic differ (multiplicative MP loss, once-per-game motive rungs, type-columned results). One field is added:

```rust
// TrackedMech, in the AS block, all #[serde(default)]:
pub bf: BfLive,

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BfLive {
    pub engine: u8,          // 'Mech/Vehicle/Aero engine hits (effects per column, §1.4)
    pub fire_control: u8,    // +2 each
    pub mp_lost: u32,        // accumulated MP-crit loss (applied-at-crit-time, §1.2); MP crits
                             // ONLY — engine MV/TP effects derive live (§1.4)
    pub weapon: u8,          // −1 damage each
    pub crew_stunned: bool,  // vehicles; next-turn no-attack flag (cleared by `n` new round)
    pub motive: BfMotive,    // vehicles; independent once-per-game spent-flags (§1.2)
    pub arm_spent: bool,     // ARM: first-crit-of-scenario ignored, marked spent (p.143)
    pub killed: Option<BfKill>, // Ammo | HeadBlownOff | CrewKilled | Fuel | ProtoDestroyed | Engine2
}
```

Destruction predicate `bf_destroyed(mech)` = structure gone (`as_struct_remaining == 0`) or `killed.is_some()` or engine ≥ 2 on the destroy-at-2 columns. Live readouts (Session methods mirroring `sbf_unit`-style derivation — computed each frame, never stored): `bf_current_mp(i)`, `bf_live_tmm(i)`, `bf_current_damage(i, range)`, `bf_shutdown(i)` (= `as_heat >= 4`, reuse `as_shutdown`).

### 2.3 Unit grouping state

```rust
// Session, append (mirrors the SBF grouping state):
#[serde(default)] pub bf: BfState,

pub struct BfState { pub units: Vec<BfUnitState>, pub active_unit: usize, pub round: u32 }
pub struct BfUnitState {
    pub name: String,
    pub elements: Vec<usize>,   // indices into Session.mechs — the pool IS the roster (SBF precedent)
    pub size: i64,              // static Unit Size, stamped at grouping time (p.53); restamped on membership edit
    pub morale: BfMorale,       // Normal | Broken | Routed — manual rung (per the SBF morale ruling)
    pub notes: String,          // the printed sheet's per-Unit Notes field (p.322)
}
```
`round` advances via `n` (new round: `round += 1`, clears every element's `crew_stunned` flag — the one piece of turn-scoped state; SBF `begin_round` precedent).

Only grouping + live counters persist; every stat line is derived per frame (the `ov_card()` doctrine). `remove_mech` remaps `bf.units[*].elements` exactly as it does SBF's (same code path, second walk). Grouping ops clone the SBF surface with one level less nesting: `bf_new_unit`, `bf_rename_unit`, `bf_remove_unit`, `bf_assign_element`, `bf_element_assignment`, `bf_prune_empty_units`, `bf_group_doctrine` (reuse `SbfDoctrine` — IS 4 / Clan 5 / CS-WoB 6 matches the printed ground-sheet capacity, **aero units pair off at 2** (Air Lance/Point, §1.7), aero never mixed with ground, p.52 — plus the itemized destructive-regroup confirmation). Ungrouped elements are legal (single-element Units are first-class in BF, p.51); the sheet renders them in an implicit "Unassigned" section rather than forcing a group.

---

## TUI

The BF screen mirrors the AlphaStrike screen throughout — this mode is AS mode's sibling, not SBF's.

### 3.1 Wiring

BF is a `Screen::BattleForce`, reached from both mode→screen matches, on the undo whitelist, with `handle_key` dispatching to `bf_key`. The Sessions screen creates one with **`F`** (a new-session key + prompt), display name "BattleForce", tag **`BF`** (with its own color); the unit picker allows AS-only units; forcegen filters candidates and prices via skill-adjusted PV (`pv`); `limit_unit` labels the cap "PV". The view keeps the force sidebar (AS mode has it), renders PV/BV at the usual sites, a BF footer key list, `unit_condition` via `bf_destroyed` + damaged predicates, and the skills/add-unit modals.

### 3.2 Panes — the AS card grid, grouped by Unit

`draw_battleforce` = the AS adaptive card grid (`as_card_lines` as the template) with:
- **Unit header rows** between groups: `▸ Fire Lance   MV 3 (j—)  SZ 2  PV 187  [Broken]` — live `bf_unit_mv`, static size, summed skill-adjusted PV, morale rung glyph, `CANNOT MOVE (shutdown)` badge when a **surviving** member is shut down (destroyed members never pin — the p.52 rule is over surviving elements). Ungrouped elements under `▸ Unassigned`.
- **Card deltas vs AS**: MV renders hex-native always (no `1` toggle — BF is hex-native; the key is freed), with live current MP when degraded: `MV 4→2 (heat 1, MP crit)`; TMM row `TMM 2 (live 1)` when degraded; damage row shows post-weapon-crit values and derived ground E (`E(9-10) L−1`); range-label footer uses `bf_range_label`; crit row renders the BF vocabulary (`Eng1 FC2 MP−4 Wpn1 CrewStun MOT:½`); threshold row for aero (`TH 5 — crit if single hit > 5`).
- **To-Hit row** per bracket via `bf_to_hit` with the persisted shot context (dash where no damage).

### 3.3 Keybinds

AS-mode verbs are unchanged: `Space`/`u` damage/repair (armor→structure, `as_damage`), `o`/`i` heat ±1, `a` add element, `,`/`.`/`[`/`]` (±1) and `<`/`>` (±4 page) navigate, `S` sessions, `b` limit, `z` undo, `L` log, `?` help. **One deliberate deviation:** AS binds `g` to the skills modal; BF reassigns `g` to the grouping editor (SBF muscle memory) and moves skills to `s`. Mode-specific:
- `c` — **crit modal** (`Modal::BfCrit`): shows the element's column of the p.42 table as dim reference (SBF `sbf_crit_modal_lines` pattern), the player enters the 2D6 result (or picks the effect directly), `apply_crit` resolves per §1.4 — ammo outcome auto-selected from CASE/CASEII/ENE; motive sub-entry for vehicles (`bf_motive_effect` reference). One undo step.
- `t` — **shot modal** (`Modal::BfShot`, mirrors `sbf_shot_modal_key`/AS `as_to_hit` editor): attacker-move 3-way, range bracket, attack kind (standard / indirect / rear-weapons / physical / air-to-ground picker, eligibility-gated per §1.3/§1.5), OV commit (bounded, shows damage delta, OVL-aware), toggles (secondary/area/spotting, strikes-rear for strafe/strike — §1.5), hand-entered target block (TMM, jumped, immobile, type, terrain, STL). Live TN + damage preview per §1.3/§1.6 (all damage previews share `Session::bf_current_damage`'s engine leg); warning lines for ineligible kinds and DFA-vs-airborne (§1.3); persists on the card like SBF's.
- `g` — **grouping editor** (`Modal::BfGroup`, clone of `sbf_group_modal_key` minus the formation level): assignment stops = units + new-unit + unassign; `a` doctrine auto-group with itemized confirm.
- `m` — Unit morale rung cycle (Normal → Broken → Routed → Normal), on the active unit.
- `n` — new round (`round += 1`, clears crew-stunned flags; §2.3).
- `r` — rename unit; `s` — skills modal (see deviation note above).

Every keybind is documented in `bf_help_modal_lines`, the `BF_HELP` footer, and `docs/keybindings-cheatsheet.html` with the session-browser `F` row and subtitle, kept in sync with the rendered `docs/neurohelmet-keybindings.pdf`.

### 3.4 Log & export

`LogEntry` carries `#[serde(default)] bf: BfState` (precedent in `log.rs`); `export.rs::render_turn` renders BF entries through the real screen (mode-parity with the SBF game log); old logs without the field parse. Any entry with `bf != BfState::default()` renders the BF screen — a zero-Unit roster gets a single frame paging the implicit Unassigned section, since all-unassigned BF sessions are first-class per §2.3; only genuinely pre-`bf`-field logs (`bf == default`) fall back to per-element AS cards.

A smoke-test checklist ships at `docs/standard-bf-smoke-test.md` (keystroke checkboxes, *Expect:* lines, one-`z`-undo invariant, a "Known non-goals" section naming apply-immediately timing, manual heat cooldown, manual morale, and no board).

---

## Rulebook conflicts & known limitations

1. **Grounded-DropShip attacker modifier: −1 vs −2.** The standard to-hit table prints −1 (both printings, p.39/p.345); the ground-to-ground prose says *"gain a −2 to-hit modifier … as shown on the To-Hit Modifiers Table"* (p.46) and the advanced table prints −2 (p.85). The mode uses **−2** (prose + advanced + AS:CE parity). Unreachable in the baked catalog; the constant is documented either way.
2. **Physical to-hit rows: standard vs advanced chapter.** Standard: Charge +1, DFA +1 (p.39). Advanced: Charge +2, DFA +3, Melee +1, Standard +0 (p.85). Not marked as a rules change anywhere. The mode uses the **standard-chapter values (+1/+1, Melee/Standard +0)** — this is the Standard-rules mode. Unresolved in the book; may change if errata clarifies. NEEDS RULEBOOK ERRATA.
3. **Attacker standstill −1 has no AS analogue** — implemented per BF (p.39). Recorded so nobody "fixes" the divergence against AS:CE.
4. **No to-hit floor.** AS mode floors at 2 (`alpha_strike.rs`); BF states none. Implemented floorless. NEEDS RULEBOOK (errata may add one).
5. **Baked TMM vs bracket-derived TMM.** `bf_tmm(inches_to_hexes(mv))` equals the baked TMM for **all ground elements** (zero mismatches); the expected MASC/quirk divergences do not exist in the baked data. Live TMM derives from the bracket table unconditionally. A catalog-sweep test pins the parity at 0 (skips with an eprintln if `data/mechs.bin` is absent; population-floor asserts — ground ≥ 8,000 / total ≥ 9,000 — keep the pin from going vacuous over a filtered bake).
6. **Odd-inch MV.** No catalog element has an odd-inch MV, so `div_ceil(2)` and "divide by 2" never diverge. Guarded by a sweep test alongside limitation 5's.
7. **Leap/intentional-fall damage contradiction** (p.72 body "1 point" vs p.74 fn16 "1 per 3 levels"): board-side movement rules, out of scope — recorded because the crit modal's "+1 damage instead" path is adjacent.
8. **Artillery.** ART* elements render their specials; the artillery attack types (direct +4 / indirect +7, registered-hex auto-hit, flight-time countdown, p.87–89) are Advanced material and out of v1. The registered-hex flag and flight-time countdown are trackable without a board — a candidate for v2 if artillery-heavy forces show up in play.
9. **Conventional infantry.** BF CI carries Gunnery + Anti-'Mech skills (p.26) and the generic-CI sheet scales damage by surviving troopers; neurohelmet bakes real CI elements with one skill and fixed damage lines. v1: one skill, fixed lines, armor pips = the baked armor (BF treats CI armor ≈ troopers; the baked AS armor already encodes it). NEEDS DECISION only if generic-CI play is ever wanted.
10. **Morale check automation.** Per-Unit Normal/Broken/Routed is manual (SBF morale ruling inherited). The check tables (trigger: element destroyed this turn; TN by experience × type; recovery with LEAD −tier) are preserved in Appendix A, not implemented. Unresolved in the book itself: the "— = exempt" reading; the immune-but-forced-check TN gap (inferno/cruise/orbital, p.97); and a **two-printing TN conflict** — the chapter table (p.98) prints Really Green 5/7/10/11 as the *Morale check* TNs, while the p.349 quick-ref prints those as the *Recovering Nerve* TNs and gives check TNs one higher (6/8/11/12). Appendix A carries the chapter values with the conflict flagged. NEEDS RULEBOOK ERRATA.
11. **Aerospace angle-of-attack.** The standard chapter carries nose/sides/aft only in a diagram the extraction dropped; values (+1/+2/+0) are sourced from the advanced-table text (p.86 fn10). Implemented as the 3-way in the shot modal; worth verifying against a printed copy.
12. **HT vs non-heat-tracking targets** (p.148: heat converts to damage; AS: no effect) — surfaced as a note row in the shot modal when the attacker has HT and the hand-entered target is flagged no-heat-scale. Damage entry stays manual either way.

---

## Cross-cutting notes

- **No bake change.** The mode consumes `AsStats` as baked; `crates/bake` is untouched (zero GameMode references).
- **Schema stability.** Every added field is `#[serde(default)]` and only enum variants are appended, so pre-existing sessions and logs load unchanged.
- **Keybinding sync.** Keybinding changes stay in sync across `bf_help_modal_lines`, the `BF_HELP` footer, and `docs/keybindings-cheatsheet.html` with its rendered PDF.
- **First-class mode.** Standard BF is a full GameMode with live tracking, not a read-only view.

---

## Appendix A — Morale check reference (NOT implemented; manual rungs only)

Per-Unit; states Normal → Broken → Routed (pp.97–99). Trigger: End Phase of any turn in which the Unit lost an element (p.98); separated/lone elements check on armor-gone and on structure damage — lone **conventional infantry** on *any* damage (p.98); inferno/cruise-missile/orbit-to-surface force checks even on "immune" types (p.97). Check: 2D6 ≥ TN passes; failure while Broken → Routed; Routed has no recovery (flees).

Base TN by Unit experience × predominant type (chapter table p.98; — = exempt. **Two-printing conflict:** the p.349 quick-ref prints check TNs one higher and labels this series Recovering Nerve — limitation 10):

| Experience | 'Mechs* | Vehicles† | Infantry** | Support‡ |
|---|---|---|---|---|
| Really Green | 5 | 7 | 10 | 11 |
| Green | 3 | 5 | 8 | 9 |
| Regular | 1 | 3 | 5 | 6 |
| Veteran | — | 1 | 3 | 4 |
| Elite | — | — | 1 | 1 |
| Legendary/Heroic | — | — | — | — |

\*incl. OmniMechs, aerospace fighters, ProtoMechs. †incl. conv. fighters, Small Craft, DropShips, WarShips. \**incl. BA. ‡incl. military SVs, JumpShips, Space Stations.

Modifiers (p.98): inferno +1 (+3 infantry), artillery +2, cruise missile +2, orbit-to-surface +4, Broken +1; infantry-only (once each): 'Mech attack +1, single element +1, in building −2, is BA −2. Recovery ("Recovering Nerve", p.99): Broken units retry each End Phase at Morale TN + recovery modifiers; LEAD element within 6 hexes **and LOS**: −tier. Broken: must withdraw toward home edge, no spotting, +1 future checks. Routed: no attacks, flees at full speed.
