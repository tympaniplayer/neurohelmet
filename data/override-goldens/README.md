# Override conversion — reference data

Reference material for the **Override** card converter (`crates/core/src/engine/override_conv.rs`),
which reproduces DFA Wargaming's client-side conversion math (damage `ceil(TW/3)`, heat `round(ΣTW/5)`,
pre-baked range brackets, a 9-slot greedy TIC packer with a ≤5 base-damage cap).

## What's here

- `reference/override_weapons.json` — the weapon DB the converter needs at runtime (weapon → damage /
  heat / range brackets), `include_str!`-embedded into the binary.
- `reference/weapon_alias.json` — weapon-name normalization used alongside it, also embedded.

Both are used by the shipped converter and are covered by DFA's permission to include Override.

## Golden values live in the tests

The converter is validated by golden cases in `override_conv.rs`'s test module. Those expected values
are **transcribed inline** from the reference conversions — the tests don't read any files here beyond
the two weapon DBs above.

The original capture corpus (MegaMek `.mtf`/`.blk` inputs and screenshots of DFA-converted cards) was
**reference-only scaffolding** and was removed before open-sourcing: it wasn't loaded by any code, and
its useful values already live in the tests. It can be regenerated locally from a MegaMek clone + the
DFA converter if the golden set ever needs extending.
