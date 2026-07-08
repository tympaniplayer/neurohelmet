# Abstract Combat System

The **Abstract Combat System** (ACS) from *Interstellar Operations: BattleForce* — the multi-regiment,
planetary-invasion scale. Create an ACS session with **`C`**.

## Structure

Elements fuse upward through the ACS hierarchy:

- **Elements** combine into **Combat Units**.
- **Combat Units** combine into **Formations**.

## What it tracks

At this scale a formation is handled as an armor pool rather than individual sheets:

- **Armor pools** with **damage thresholds** — the formation absorbs damage until a threshold breaks.
- **Fatigue** — accumulated wear across the engagement.
- **Morale** — tracked manually.

ACS is **ground-only** in this version, and is costed in **PV**. Press **`?`** for the ACS keymap.
