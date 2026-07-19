# Keybindings

Every key in Neurohelmet, in one place. Two things to know before you scan the tables:

- **Keys are case-sensitive.** `d` (toggle prone) and `D` (delete unit) are different commands;
  capitals need Shift.
- **The in-app help is the authoritative reference.** Press **`?`** in any mode for that mode's
  hand-maintained keymap, straight from the app you're running. There's also a printable one-page
  [cheat sheet PDF](https://github.com/tympaniplayer/neurohelmet/blob/main/docs/neurohelmet-keybindings.pdf)
  in the repository covering every mode.

The `?` help exists on all six play screens. The unit picker and Sessions browser don't have it —
they carry a persistent hint line at the bottom instead.

## Keys shared across every mode

These work the same on all six play screens:

| Key | Action |
|-----|--------|
| **`?`** | help — the current mode's keymap (any key closes it) |
| **`a`** | add units — opens the [unit picker](../guides/force-generation.md) |
| **`S`** | [Sessions browser](../guides/sessions.md) |
| **`z`** | undo, 50 deep — never crosses session boundaries |
| **`Space`** / **`Enter`** | the mode's primary verb — damage a location, fire a weapon, open the damage input |
| **`u`** | the primary verb's opposite — repair, un-fire, refill |
| **`q`** | quit, with a y/n confirm |
| **`Ctrl+C`** | quit immediately, from anywhere — even inside a popup |
| **`Ctrl+T`** | [display picker](../guides/display.md) (theme / layout / icons), on every screen — but not while a popup is open |
| **`L`** | [game-log snapshot](../guides/game-log.md) — every mode **except ACS**, where `L` is an aerospace readout key and there is no game log |

A few jobs exist everywhere but sit on different keys per mode — the price of six systems sharing
one keyboard:

| Job | Classic | Alpha Strike | Override | BF | SBF | ACS |
|-----|---------|--------------|----------|----|-----|-----|
| Cycle active unit | `[` `,` / `]` `.` | `[` `,` / `]` `.` | `[` `,` / `]` `.` | `[` `,` / `]` `.` | `↑↓` / `kj` (units) | `↑↓` / `kj` (Combat Units) |
| Cycle formations | — | — | — | — | `[` `,` `Shift+Tab` / `]` `.` `Tab` | `←` `h` `,` / `→` `.` |
| Edit skills | `g` | `g` | `g` | **`s`** | `s`/`S` inside the `g` editor | — |
| Group editor | — | — | — | `g` | `g` | `g` |
| Point limit | `b` (BV) | `b` (PV) | picker `Ctrl+B` only | `b` (PV) | `b` (PV) | picker `Ctrl+B` only |
| End turn / round | `e` | — (no end turn) | `e` | `n` | `n` round · `e` done | `n` round · `e` done |
| Delete | `D` unit | `D` unit | `D` unit | `D` element | `D` formation (confirms) | `D` formation (no confirm; `z` undoes) |
| [PDF record sheet](../guides/pdf-record-sheets.md) | — | — | — | `P` | `P` | `P` |

There is **no save key** anywhere — Neurohelmet autosaves after every change.

## Classic

The [Classic tracker](../modes/classic.md) has two focus targets — the paper doll and the
equipment list — and several keys act on whichever is focused.

| Key | Action |
|-----|--------|
| **`Space`** / **`Enter`** | doll: 1 point of damage to the cursored location · equipment: fire the weapon / spend a round of ammo |
| **`u`** | doll: repair 1 (internal structure first) · equipment: un-fire / refill a bin |
| **`Tab`** | switch focus doll ↔ equipment |
| **`↑↓←→`** / **`kjhl`** | move the doll cursor / scroll the equipment list |
| **`f`** | toggle Front/Rear facing |
| **`o`** / **`i`** | heat +1 / −1 |
| **`e`** | end turn for the **active unit**: heat dissipates; fired marks, movement, the per-turn damage tally, and the GATOR target clear |
| **`d`** | toggle prone (knocked down) |
| **`x`** | toggle shutdown / restart |
| **`X`** | toggle pilot unconscious |
| **`p`** / **`P`** | pilot hit / heal (crew on vehicles) |
| **`c`** | critical-slots popup (vehicles and aerospace get their crit-result lists instead) |
| **`t`** | GATOR to-hit target |
| **`v`** | movement this turn (aerospace: velocity and altitude) |
| **`r`** | dice reference (cluster hits, hit locations) |
| **`m`** / **`M`** | motive-damage popup / quick-repair the last result (vehicles only) |
| **`J`** | toggle equipment state: jam an Ultra/Rotary AC, engage MASC/Supercharger, ECM/Stealth on/off |
| **`g`** | pilot skills |
| **`b`** | force BV limit |
| **`[`** `,` / **`]`** `.` | previous / next unit (`Shift+Tab` also goes back) |

## Alpha Strike

The [card screen](../modes/alpha-strike.md) — note there is **no end-turn key**: heat is a manual
dial and the shot context persists until you change it. `L` is your turn marker.

| Key | Action |
|-----|--------|
| **`Space`** / **`Enter`** | 1 damage (armor first, then structure) |
| **`u`** | repair 1 (structure first) |
| **`o`** / **`i`** | heat +1 / −1 on the `0 1 2 3 S` dial |
| **`c`** | crit popup (the types offered vary by unit type) |
| **`t`** | to-hit shot editor (attacker jump, target TMM/jumped/immobile) |
| **`1`** | toggle 1:1 ground (hex) scale |
| **`g`** | edit the single Skill |
| **`b`** | force PV limit |
| **`[`** `,` / **`]`** `.` | previous / next unit |
| **`<`** / **`>`** | jump 4 units — one card-grid row |

## Override

The [Override tracker](../modes/override.md) mirrors Classic's doll-plus-panel layout; `Space`
fires TICs and banks their heat. There's no `b` here — set the BV limit at session creation or
with `Ctrl+B` in the picker.

| Key | Action |
|-----|--------|
| **`Space`** / **`Enter`** | doll: 1 pip of damage to the cursored region · weapons: fire the selected TIC (banks its heat) |
| **`u`** | doll: repair 1 pip · weapons: un-fire (refunds heat) |
| **`Tab`** | switch focus doll ↔ weapons panel |
| **`↑↓←→`** / **`kjhl`** | move the region cursor / cycle TIC selection |
| **`f`** | toggle front/rear facing |
| **`o`** / **`i`** | heat +1 / −1 on the 0–5 ladder (vehicles have no heat track) |
| **`e`** | end turn: heat dissipates; TIC fired marks, movement, and the damage tally clear |
| **`x`** | toggle shutdown / restart |
| **`c`** | per-region crit popup (inside it: `Space` marks a hit, `Backspace` removes one, `a` toggles the region's ammo live/spent) |
| **`t`** | to-hit shot editor (target TMM, jumped, immobile, secondary, rear arc) |
| **`v`** | movement this turn (aerospace: velocity and altitude) |
| **`g`** | pilot skills |
| **`p`** / **`P`** | pilot / crew hit and heal |
| **`[`** `,` / **`]`** `.` | previous / next unit (`Shift+Tab` also goes back) |

## BattleForce

[Standard BattleForce](../modes/battleforce.md) keeps the Alpha Strike verbs and adds lance-Unit
management. Watch the deliberate swap: **`g` is the group editor here, `s` is skills.**

| Key | Action |
|-----|--------|
| **`Space`** / **`Enter`** | 1 damage to the active element (armor then structure) |
| **`u`** | repair 1 |
| **`o`** / **`i`** | heat +1 / −1 (manual dial; cooldown is manual too) |
| **`c`** | BattleForce crit modal — pick the row your 2D6 landed on (inside it: `a` marks the ARM chance spent) |
| **`t`** | to-hit shot editor (the full p.39 modifier table) |
| **`g`** | group-force editor (inside it: `←→` move between Units, `n` split, `u` unassign, `s`/`S` element skill, `x` remove, `a` doctrine auto-group) |
| **`m`** | cycle the active Unit's morale: Normal → Broken → Routed |
| **`n`** | begin a new round (clears crew-stunned) |
| **`r`** | rename the active Unit |
| **`s`** | pilot skills (single Skill row) |
| **`b`** | force PV limit |
| **`[`** `,` / **`]`** `.` | previous / next element |
| **`<`** / **`>`** | jump 4 elements — one card row |
| **`P`** | export the [PDF record sheet](../guides/pdf-record-sheets.md) |

## Strategic BattleForce

[SBF](../modes/strategic-battleforce.md) is a three-pane screen: formations left, units middle,
detail right — so selection keys split between the two levels.

| Key | Action |
|-----|--------|
| **`[`** `,` **`Shift+Tab`** / **`]`** `.` **`Tab`** | previous / next formation |
| **`↑`** `k` / **`↓`** `j` | previous / next unit within the formation |
| **`Space`** / **`Enter`** | 1 damage to the active unit — overflow spills onto a unit you pick |
| **`u`** | repair 1 armor |
| **`c`** | crit-counter popup (Damage / Targeting / MP crits) |
| **`t`** | to-hit editor (range, jump, target TMM, terrain, aero and capital rows) |
| **`m`** | cycle formation morale: Normal → Shaken → Broken → Routed |
| **`n`** | begin round (clears every ✓ done mark, resets jump) |
| **`e`** | mark the active formation done this turn |
| **`g`** | group-force editor (inside it: `←→` move, `n` split to a new unit, `f` new formation, `u` unassign, `s`/`S` element skill, `x` remove, `a` doctrine auto-group) |
| **`r`** / **`R`** | rename the active formation / unit |
| **`C`** | toggle the active unit as Force Commander (COM) |
| **`l`** | toggle the active unit as Formation Leader (LEAD) — not a navigation key here |
| **`b`** | force PV limit |
| **`D`** | delete the active formation **and its elements** (y/n confirm) |

## Abstract Combat System

[ACS](../modes/abstract-combat-system.md) tracks Combat Units inside Formations. It has **no game
log** — its printable artifact is the [PDF record sheet](../guides/pdf-record-sheets.md) — and a
handful of keys only appear for aerospace Formations.

| Key | Action |
|-----|--------|
| **`←`** `h` `,` / **`→`** `.` | previous / next Formation |
| **`↑`** `k` / **`↓`** `j` | previous / next Combat Unit |
| **`Space`** / **`Enter`** | open the damage input for the active Combat Unit (type a number, `Enter`) |
| **`u`** | repair 1 armor |
| **`m`** / **`M`** | cycle the Combat Unit's / the Formation's morale rung (seven rungs, wraps) |
| **`f`** / **`F`** | fatigue: mark "fought this turn" (+FP) / rest (−1 FP) |
| **`n`** | begin next round |
| **`e`** | toggle the active Formation's done mark |
| **`g`** | group-force editor (inside it: `←→` move between SBF Units, `n`/`t`/`c`/`F` split to a new Unit / Team / Combat Unit / Formation, `u` unassign, `a` auto-group) |
| **`r`** | rename the active Formation |
| **`D`** | delete the active Formation — no confirm; its elements return to the pool, and `z` undoes it |
| **`C`** / **`l`** | set Force Commander (COM) / Formation Leader (LEAD) |
| **`[`** / **`]`** | cycle the readout range — Short/Medium/Long, plus Extreme for aerospace Formations |
| **`+`** `=` / **`-`** `_` | target TMM up / down |
| **`s`** | toggle secondary target |
| **`P`** | export the [PDF record sheet](../guides/pdf-record-sheets.md) |

Aerospace Formations only:

| Key | Action |
|-----|--------|
| **`w`** | cycle capital weapon class |
| **`v`** | cycle firing arc |
| **`x`** | cycle the cross-type matchup (aero → WarShip, etc.) |
| **`L`** | toggle "target is a large craft" (waives the weapon-class penalty) |
| **`y`** | cycle the Ground-Support mission |

## Unit picker

Letters type into the fuzzy search — which is why the picker leans on `Ctrl` for everything else
(and why `j`/`k` don't navigate here). Full tour in [Building a force](../guides/force-generation.md).

| Key | Action |
|-----|--------|
| any letter | type into the search query (`Backspace` deletes) |
| **`↑`** / **`↓`** | move the selection (`PageUp` / `PageDown` jump a page) |
| **`Tab`** | toggle the unit preview popup |
| **`Enter`** | open the Add-unit modal — set skills with `←→` (right improves), `Enter` commits |
| **`Esc`** | close the preview, else back to your tracker, else (empty roster) the Sessions browser |
| **`Ctrl+F`** | faceted filters (`↑↓`/`kj` facet, `←→`/`hl` cycle value, `c` clears all, `Enter` on Faction opens a search box) |
| **`Ctrl+B`** | set the force point limit |
| **`Ctrl+G`** | [random force generator](../guides/force-generation.md) (`↑↓` field, `←→`/`Space` change, `Enter`/`r` roll; then `Enter` accept, `r` reroll, `Backspace` back) |

## Sessions browser

Opened with **`S`** from any play screen. Here `q` means *back*, not quit. Details in
[Sessions & autosave](../guides/sessions.md).

| Key | Action |
|-----|--------|
| **`↑↓`** / **`kj`** | select a session |
| **`Enter`** | load it (loading the active session reports "Already active") |
| **`n`** | new Classic session |
| **`A`** | new Alpha Strike session |
| **`O`** | new Override session |
| **`B`** | new Strategic BattleForce session |
| **`F`** | new BattleForce session |
| **`C`** | new Abstract Combat System session |
| **`r`** | rename (pre-filled with the current name) |
| **`D`** | delete, with confirm — the active session can't be deleted |
| **`Esc`** / **`q`** | back to where you were |

## Inside popups

Every list-style popup follows the same conventions:

- **`↑↓`** / **`kj`** select a row; **`←→`** adjust where the row is numeric; **`Space`** /
  **`Enter`** toggle or apply.
- **`Esc`** closes — and so does **the key that opened the popup** (`c` closes the crit popup,
  `t` the shot editor, `g` the skills or group editor, `r` the dice reference, `m` the motive
  popup).
- Confirm prompts take **`y`** / **`Enter`** for yes, **`n`** / **`Esc`** for no. Typed inputs
  take **`Enter`** to accept, **`Esc`** to cancel.
- The Classic dice reference pages with **`Tab`** / **`→`** / **`l`** forward and
  **`Shift+Tab`** / **`←`** / **`h`** back.
- The Classic critical-slots popup adds **`a`** (set the active ammo bin) and **`t`** (load a
  different munition).
- Popups capture all input while open; **`Ctrl+C`** still quits.
