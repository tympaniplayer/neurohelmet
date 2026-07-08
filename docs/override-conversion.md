# MegaMek → Override card conversion

How to turn a parsed BattleTech unit into a **BattleTech: Override** record card (DFA Wargaming's
fan ruleset). This is the implementation spec for neurohelmet's Override support, validated against
10 golden cards (`data/override-goldens/`).

> **Attribution.** Override is a fan ruleset by Death From Above Wargaming
> (`dfawargaming.com/override-cards`). The `_V5` suffixes in the algorithm names track Override
> rules **v5.x** (current docs: v5.3r1).

---

## 1. The card

```
UNIT DATA            HEAT SCALE (mech/aero only)     ARMOR DIAGRAM (per-location pips)
  Type   Mass          5 Automatic Shutdown
  Move w/r/j           4 Ammo Explosion (avoid 8+)
  TMM  w/r/j           3 Shutdown (avoid 8+)
  Sinks (mech/aero)    2 +1 Ranged Attack Mod
  Thrust/DThr (aero)   1 -2 Ground Move|Safe Thrust / -1 TMM
                       0 No Effects
WEAPONS  Dmg [Ht] Loc PB S M L X        ← Ht column present for mech+aero, ABSENT for vehicles
  …one row per TIC…
  Punch / Kick  (mech only)
Equipment: <engine type>, <CASE/AMS/…>, Ammo:<type> (<loc>)
Condition Monitor: 3+ 5+ 7+ 9+ 11+ KIA
```

A unit's offense is a list of **TICs** (fire groups). Each TIC is one weapons row. Weapons are
auto-grouped into ≤9 TICs by a packing algorithm (§4).

Per-unit-type differences (validated against goldens):

| | Ht col | Heat scale | Sinks | Punch/Kick | Extra | Locations |
|---|---|---|---|---|---|---|
| **Mech** (biped) | ✓ | ✓ | ✓ | ✓ | Engine/Gyro boxes | HD, LA/RA, LT/CT/RT (6,7,8), LL/RL, Torso Rear |
| **Quad mech** | ✓ | ✓ | ✓ | ✓ | — | HD, Torso, 4 legs (FL/FR/RL/RR), Torso Rear; **no arms** |
| **Vehicle** | ✗ | ✗ | ✗ | ✗ | — | Front (6,7,8), Turret (5,9), L/R Side, Rear |
| **VTOL** | ✗ | ✗ | ✗ | ✗ | — | Front, L/R Side, **Rotor (5,9)**, Rear |
| **Aerospace** | ✓ | ✓ | ✓ | ✗ | Thrust, **DThr** | Nose (6,7,8), L/R Wing, Aft (2,12) |

---

## 2. Weapon database

`data/override-goldens/reference/override_weapons.json` — 240 entries, keyed by internal id
(`mlas`, `ppc`, `lrm20`, `clb10x`, `chag40`, …). Embed this in the binary. Fields:

| field | meaning |
|---|---|
| `name` | display name (`"MLas"`, `"UAC/5"`, `"cERLLas"`) |
| `fullname` | match key against the parsed unit's weapon name (`"Medium Laser"`) |
| `tech` | `IS` / `Clan` / `Mixed` |
| `type` | `E` energy, `B` ballistic, `M` missile, `P` physical |
| `damage` | **TW damage** → Override base via `ceil(/3)` |
| `heat` | **TW heat** → TIC heat via `round(Σ/5)` |
| `crits` | TW crit slots (used for crit confirmation, not the card row) |
| `rangePB/S/M/L/X` | **pre-baked Override bracket modifier.** `0/2/4…` = `+N`; **`9` (or any >6) = "—" / no shot**. Negative (pulse `-2`) allowed. |
| `damageM` | count of **M (missile/cluster) dice** |
| `shiftM` | M-dice adjustment subtracted during grouping |
| `damageAdj` / `damageMax` | max damage for the `(n)` cap in `+M..(n)` |
| `varPBSdamage` `varMdamage` `varLXdamage` | variable-by-bracket damage (SNPPC, MML, HAG) → `a\|b\|c` |
| `useC` | cluster weapon (LB-X, HAG) → `+C` notation |
| `useH` | heat-damage weapon (flamer) → `+H` |
| `useR` | rapid-fire (UAC/RAC) → `(RF)` tag; `useR` value is the rack/shots figure |
| `useFCS` | compatible fire control, csv: `aiv` (Artemis IV), `av` (Artemis V), `apollo` |
| `useAmmo` | ammo type key (`LRM`, `Gauss`, `HAG`, …) |
| `useTC` | targeting-computer eligible |
| `specials` | csv tags: `lrm srm mrm atm rl srt lrt var hag20..40 ai zerobase ssrm slrm sbg` |
| `baOnly` | battle-armor-only weapon |

> The DB is the **single source of truth for per-weapon Override stats.** Do not recompute range
> brackets from TW ranges — DFA precomputed them here (incl. pulse −2, min-range penalties). Match
> each parsed `WeaponMount` to a DB entry by `fullname` (with a normalization/alias table for
> naming drift between Mekbay and DFA).

---

## 3. Unit-level conversion

Ground truth: DFA's `destinyArmorValue` / `destinyStructureValue` / `damageThreshold` getters + the
card's Move/TMM/Sinks render. Ported in `override_conv.rs` (`unit_data`, `override_armor`,
`aero_dthr`), golden-tested against all 10 cards.

- **Move** `walk / run` (+ ` / Nj` when it jumps) = TW MP verbatim (1" = 1 hex). Suffix the **run**
  figure with the motive code for vehicles (`6t`, `14v`; only `t`/`v` are golden-pinned). Aero shows
  a single **Thrust** = Safe Thrust; vehicles drop the heat ladder, aero keeps it.
- **TMM** = the TW target-movement bracket of each move value (`0-2→0, 3-4→1, 5-6→2, 7-9→3, 10-17→4,
  18-24→5, 25+→6`), **+1 when jumping that value, +1 when airborne (VTOL / aerospace)**. So jump-MP 4
  → bracket 1 + 1 = TMM 2, while walk 4 → 1. *(This is `engine::movement::target_movement_modifier`
  plus the airborne term — not the printed AS TMM, which is a single per-unit value.)*
- **Sinks** = `round(dissipation / 5)` (mech/aero only; same TW/5 scale as the heat ladder). E.g.
  13 single HS → 3; 17 double (34) → 7.
- **Heat scale** is the fixed 0–5 ladder above; mech & aero only. Level-1 label is
  `-2 Ground Move / -1 TMM` for ground, `-2 Safe Thrust / -1 TMM` for aero.
- **Aerospace DThr** = `max( round( ((LW+RW)/2 + Nose + Aft) / 30 ), 1 )` over **raw TW** arc armor.
  ⚠️ The Stuka STU-K5 golden card prints **5**, but this formula yields **6** for its 15-ton armor
  (84/54/54/48 → 186/30 = 6.2 → 6). A 1-point discrepancy — possibly a stale capture or an SI
  interaction — left as a follow-up; the port emits the literal formula (6).
- **Armor** — every pip = `max( round(a / t), 1 )`:
  - Mech head: `a` = TW→pip table `≤2→1, ≤5→2, ≤7→3, else 4`, `t = 1`.
  - Mech torso (the three torsos merge into one region): front `a = CT+LT+RT, t = 6`; rear
    `a = CTr+LTr+RTr, t = 6`.
  - Mech limbs (arms / legs / quad legs): `a` = TW location armor, `t = 3`.
  - Vehicle / aero locations: `a` = TW armor, `t = 4`.
- **Structure** — also `max( round(a / t), 1 )`, then **halved (min 1) for Composite**:
  - Mech head `a = HD internal, t = 3`; torso `a = CT+2·ST internal (= the three baked torso
    internals), t = 7`; arms/legs `a = limb internal, t = 3`.
  - Vehicle: uniform `a = ceil(tonnage/10), t = 3`.
  - Aero: one shared SI pool, `a = max(thrust, floor(0.1·tonnage)), t = 3`.
- **Type** label: vehicle → `Combat Vehicle`, aero → `Aerospace Fighter`, mech → `OmniMech` /
  `IndustrialMech` / `BattleMech` by Mekbay subtype.
- **Condition monitor** TNs are fixed: `3+ 5+ 7+ 9+ 11+ KIA`.
- **Hit-location numbers** beside each diagram region are the fixed 2d6 front table (`HD 12, LA/LFL
  10-11, RA/RFL 3-4, torsos 6-8, LL/LRL 9, RL/RRL 5`; vehicle/aero use their own fixed tables) — a
  render constant, not unit-derived.

---

## 4. TIC packing (`autoGroupWeaponsV5`)

9 TIC slots. Greedy placement in sorted order, with a bounded displacement search.

```
fn auto_group(weapons):
    tics = [empty; 9]
    weapons.sort(sort_v5)                       # §4.1
    for w in weapons: place(w, depth=0, exclude=-1)

fn place(w, depth, exclude):
    score[t] = score_weapon_for_tic(w, tics[t], t)   for t in 0..9   # §4.2
    best = argmax(score)
    move = None
    if depth < 3:                               # try to displace a weapon for a better fit
        for t in 0..9 where tics[t].used and tics[t].len >= 2 and t != exclude:
            for s in 0..tics[t].len:
                removed = tics[t].remove(s)
                m = score_weapon_for_tic(w, tics[t], t)
                tics[t].insert(s, removed)
                if m > score[best] and (move is None or m > move.score):
                    move = {tic: t, idx: s, score: m}
    if move:
        bumped = tics[move.tic].remove(move.idx)
        w.tic = move.tic;  tics[move.tic].push(w)
        place(bumped, depth+1, move.tic)        # re-place the displaced weapon
    else:
        w.tic = best;  tics[best].push(w)
```

### 4.1 `sortV5` ordering (weapon comparator)
By: `tic` → `rangeX` → `rangeL` → `rangeM` → `rangeS` (all asc, `9` default) → `damageMax` desc →
`heat` asc → location (`LA`<`RA`<other) → `name` → rear-mounted last.

### 4.2 `_scoreWeaponForTicV5(w, tic, slot)` → number (0 = cannot place here)
```
score = 100 - slot
if tic is empty: return score                  # any weapon opens an empty TIC

# HARD GATES — return 0 if any fail:
- arc: weapon's Rear/Left/Right arc must be in tic.arcs
- if unit has TC:      w.useTC          must equal tic.useTC
- if unit has Artemis: w.useFCS(aiv/av) must equal tic.useAIV/useAV
- if unit has Apollo:  w.useFCS(apollo) must equal tic.useApollo
- if unit has AES:     w.useAES         must equal tic.useAES
- family match: each of {ssrm,slrm,srm,lrm,mrm,atm,rl,srt,lrt,hag,sbg} must agree
- rapid-fire: (w.useR>0) must equal (tic has useR)
- one-shot:   w.useOS must equal tic.useOS
- physical (type "P"): return 0
- BASE-DAMAGE CAP: combined base > 5 → return 0
    missile:  s = ceil(tic.damage/3) + tic.damageM + w.damageM
    else:     s = ceil((tic.damage + w.damage)/3) + tic.damageM
    (×squadSize for BA).  A single weapon whose own base > 5 (e.g. AC/20 → 7) still gets its
    OWN tic — the cap only blocks *grouping*, it never rejects an empty tic.

# SOFT SCORE — multiply `score` by:
- arc fit:   same arc ×1.0;  (Any weapon into a tic of all-Any/Front) ×0.8;  else ×0.6
- range similarity:  ×1 if equal else ×0.1 (X), ×0.2 (L), ×0.4 (M), ×0.8 (S)
- precision-ammo mixing ×0.8
- heat/damage rounding-efficiency term (rewards groupings where round(Σheat/5) and ceil(Σdmg/3)
  don't "waste" a point vs. summing separately)
- if destinySinks set: ×(1 - 0.2*clamp(round((tic.heat+w.heat)/5) - destinySinks, 0, 5))
return score
```

> The **range-similarity term is what makes the converter cluster weapons with identical range
> profiles** (e.g. two ER Mediums) and split unlike ones — even before the ≤5 cap kicks in.

---

## 5. Row rendering (`groupData`) — one TIC → one card row

```
name   = dedup+count weapon names ("x2 LRM-15"), joined ", ", then suffix tags:
         (TC)(AIV)(AV)(Apollo)(OS)(AES)(AI)(RF)(Precision)(Armor Piercing)(Flak)(Flechette)(Tracer)
heat   = round( Σ TW heat / 5 )                         # mech/aero only
damage:
    base = ceil(Σ plain-damage / 3) + Σ damageM - clusterC
      where clusterC = ceil(Σ cluster-weapon-damage / 3) - count(cluster weapons)
    append " +C{clusterC}"  if any cluster
    append " +H{max(round(ΣuseH/5),1)}" if any heat-damage
    append " +M{useM} ({ceil(damageMax/3)})" if any missile
    variable weapons → render as "a|b|c" (PBS|M|LX), each = ceil((Σdmg+var)/3)+damageM-clusterC
    HAG → "{hagBase}+C{a}|{b}|{c}";   flechette lost dmg → " (n + AI)"
loc    = distinct locations joined ", ";  rear weapons prefix the whole row with "(R) "
range  = per bracket: take MAX modifier across the tic's weapons, then
         −1 each if (TC | Apollo | AES) ;  −1 if precision ammo ;  +1 if AP ammo
         render "+N";  value > 6  →  "--"  (no shot in that bracket)
```

### Worked checks (from goldens)
- `AC/20` → `ceil(20/3)=7` dmg, `round(7/5)=1` heat, own TIC (7>5). ✓ *(Hunchback)*
- `x2 cLRM-20` → base 4, `+M4 (14)`. ✓ *(Mad Cat)*
- ER Large (Clan dmg 10) → `ceil(10/3)=4`; two of them = 8 > 5 → never grouped. ✓ *(Mad Cat, Stuka)*
- `LB 10-X` → `1+C3`. ✓ *(Bushwacker)*  ·  `cHAG 20` → `2+C5|4|3`. ✓ *(Vulture)*
- `MML-9` w/ Artemis → `2|2|1+M2 (5) (AIV)`. ✓ *(Crusader)*
- `RAC/5` → `ceil(8/3)=3 (RF)`. ✓ *(Rifleman)*  ·  pulse → PB `-2`. ✓ *(Mad Cat cMPLas)*

---

## 6. Known DFA quirks (do NOT replicate)
- **VTOL Mass is mis-displayed** — Warrior H-7C (`tonnage 21.0`) prints "Mass: 5 Tons". Bug in the
  reference converter; neurohelmet should show true tonnage. *(All other tonnages match exactly.)*
- **Destiny mode is dormant** in the current build — every card shows *gross* heat (`destinySinks`
  is unused). Target standard mode; leave a `net_heat` toggle as a future hook.

---

## 7. neurohelmet integration

**Approach: compute at runtime, no bake change.** The conversion is a pure, microsecond-scale
transform over data already in `Mech` (per-location `LocationArmor`, per-weapon `WeaponMount` with
TW `damage`/`range`/`heat`). No new fetch, no `mechs.bin` growth, no `BUNDLE_VERSION` bump.

- **New module:** `crates/core/src/override/` (`mod.rs`, `weapons.rs` w/ the embedded DB, `pack.rs`
  the §4 algorithm, `card.rs` the §1 output type + §5 rendering).
- **Input:** an existing `&Mech` (+ its `UnitType`/`MechConfig`/`MotiveType`). `Location` already
  has every arc Override needs (HD/torsos/quad legs/Front/Turret/Rotor/Nose/Wing/Aft).
- **Weapon match:** `WeaponMount.name`/`fullname` → DB id via an alias table. `WeaponMount.rear`
  → rear arc; `count` → the `x2` multiplier; `ammo_key` → the Equipment line.
- **Output:** an `OverrideCard { unit_data, heat_scale, tics: Vec<Tic>, armor, equipment }` the TUI
  renders as a new card mode (alongside the existing AS/Classic dolls).
- **Tests:** golden regression against `data/override-goldens/` — assert each computed `Tic`
  matches the card image's row (transcribe the 10 cards into expected fixtures).

**v1 scope:** 'Mechs (biped/quad), combat vehicles, VTOLs, aerospace fighters; standard loadouts.
**Out of scope (v1):** special-ammo/munition toggles, infantry/BA platoons, the Destiny net-heat
mode, the Strike-Ops force-builder.
```
