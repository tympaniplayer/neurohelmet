# Overview

Neurohelmet tracks six BattleTech systems, all fed by the same catalog of **9,724 units**. Each
session is **locked to one system** at creation, so pick the one that matches the game on your
table. Costing follows the system: Classic and Override balance forces in **BV** (Battle Value);
the Alpha Strike family uses **PV** (Point Value).

| System | Scale | Create | Cost | PDF sheets |
|--------|-------|:------:|:----:|:----------:|
| **[Classic](classic.md)** (Total Warfare) | one full record sheet per unit | **`n`** | BV | — |
| **[Alpha Strike](alpha-strike.md)** | one card per unit | **`A`** | PV | — |
| **[Override](override.md)** (DFA Wargaming) | one card per unit, region doll + heat ladder | **`O`** | BV | — |
| **[BattleForce](battleforce.md)** (IO:BF) | lances of AS cards, hex scale | **`F`** | PV | yes |
| **[Strategic BattleForce](strategic-battleforce.md)** (IO:BF) | formations of lances | **`B`** | PV | yes |
| **[Abstract Combat System](abstract-combat-system.md)** (IO:BF) | planetary invasion | **`C`** | PV | yes |

The **Create** keys live in the Sessions browser — press **`S`** from any play screen to open it
(on a fresh install, `Esc` from the empty unit picker offers it too). Watch the letters: **`B`**
creates a *Strategic* BattleForce session and **`F`** a standard BattleForce one. The three IO:BF
systems can also print blank, table-ready [record sheets](../guides/pdf-record-sheets.md) with
**`P`**.

## What every system shares

However far up the scale you play, the same core machinery is underneath:

- **The unit picker** — **`a`** opens it from any play screen. Search, filters, the stat preview,
  the force generator, and budgets all work identically in every system; see
  [Building a force](../guides/force-generation.md).
- **Named sessions with autosave** — every change is saved within a fraction of a second; there is
  no save key because there is nothing to save by hand. Sessions are per-system, and the
  [Sessions browser](../guides/sessions.md) tags each one (`CL`, `AS`, `OV`, `BF`, `SB`, `AC`).
- **Undo** — **`z`** steps back through the last **50** changes in all six systems. The stack
  clears when you switch sessions.
- **Force point budgets** — set an optional BV or PV limit when the session is created, or later
  with **`Ctrl+B`** in the picker. Totals are always skill-adjusted and show up in the picker
  title, the add-unit modal, and the sessions list. (Classic, Alpha Strike, BattleForce, and SBF
  also bind **`b`** on the play screen; Override and ACS set their limit at creation or via the
  picker.)
- **The game log** — **`L`** snapshots the whole force in Classic, Alpha Strike, Override,
  BattleForce, and SBF; see [Game log & publishing](../guides/game-log.md). ACS has no game log —
  its printable artifact is the PDF record sheet.
- **Roster caps** — Classic and Override cap the roster at **12** units. Alpha Strike,
  BattleForce, SBF, and ACS rosters are uncapped.
- **Help and display** — **`?`** opens each system's own key reference (the authoritative one —
  keybindings genuinely differ between systems), and **`Ctrl+T`** opens the theme/layout picker
  everywhere.

### Same key, different job

A few keys change meaning as you move up the scale — worth knowing before muscle memory bites:

- **`g`** edits pilot skills in Classic, Alpha Strike, and Override, but opens the **group
  editor** in BattleForce, SBF, and ACS. In BattleForce, skills move to **`s`**; SBF and ACS
  have no skills modal at all — SBF edits element skill inside the group editor, and ACS sets
  it once, when the unit is added.
- **`e`** ends the active unit's turn in Classic and Override (heat resolves, fired marks clear).
  Alpha Strike has no end-turn key at all — heat is a manual dial — and BattleForce drops **`e`**
  too, though **`n`** still begins its next round. In SBF and ACS, **`e`** marks the active
  formation done for the round and **`n`** begins the next one.
- **`D`** removes the active *unit* in Classic, Alpha Strike, and Override — in BattleForce the
  active *element*, never its lance — but deletes the whole active *formation* in SBF and ACS.

The complete per-system tables are in the [keybinding reference](../reference/keybindings.md).

## Choosing a system

**[Classic](classic.md)** is the full Total Warfare record sheet: armor bubbles, critical slots,
heat, ammo bins, pilot hits, GATOR to-hit math. It is the richest tracker and the slowest game —
best for the classic one-lance-a-side night where every AC/20 shell matters.

**[Alpha Strike](alpha-strike.md)** trades all of that for one card per unit: armor and structure
pips, a 0–3–S heat dial, four crit types. Games are faster and forces bigger — the roster is
uncapped for a reason. Damage is a point at a time and everything else stays in your head.

**[Override](override.md)** is Death From Above Wargaming's fast-play system, included with
permission — a middle ground between Classic and Alpha Strike. Each card keeps a region doll with
2d6 hit locations, weapon fire groups (TICs), and a 0–5 heat ladder, converted on the fly from
the same data Classic uses. See the [Override page](override.md) for the full system and
attribution.

**[BattleForce](battleforce.md)** is Alpha Strike's hex-scale sibling: you still track every
element's own card, but elements group into lance **Units** with derived movement, morale rungs,
and the IO:BF crit table. Pick it when you want company-scale games without giving up per-element
damage.

**[Strategic BattleForce](strategic-battleforce.md)** moves the tracked atom up a tier: lances
fuse into Units with a single aggregate armor pool, and Units group into Formations. Individual
'Mechs disappear into the math — that is the point. Best for battalion-scale engagements.

**[Abstract Combat System](abstract-combat-system.md)** is the top rung: whole battalions become
one Combat Unit stat line with an armor pool, damage thresholds, fatigue, and a seven-rung morale
track — the planetary-invasion game, ground and aerospace both.

Not sure? Start with Classic and the [first session walkthrough](../first-session.md) — every
habit it teaches carries up the ladder.
