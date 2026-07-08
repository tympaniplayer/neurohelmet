# Notices & attribution

Neurohelmet is an **unofficial, non-commercial, fan-made** BattleTech play aid. It is **not** affiliated
with, endorsed by, or licensed by Microsoft, The Topps Company, Catalyst Game Labs, Death From Above
Wargaming, or the MegaMek project.

## Licensing

| Part | License |
|------|---------|
| Source code (`crates/`, build tooling, docs we wrote) | **GPL-3.0-or-later** — see [LICENSE](LICENSE) |
| Bundled game data (`data/mechs.bin` + everything baked into / embedded in the binary) | **CC-BY-NC-SA-4.0** |
| Bundled font (`crates/app/assets/fonts/Roboto-*.ttf`, embedded for PDF record sheets) | **Apache-2.0** (Roboto — the face Mekbay renders its sheets with) |
| Bundled record-sheet logos (`crates/app/assets/logos/{BT_Logo_BW,CGL_Logo}.png`) | **CC-BY-NC-SA-4.0** (from MegaMek's assets; Topps/CGL trademarks — see below) |

The code is **GPLv3-or-later**, matching the MegaMek/MekHQ/Mekbay ecosystem (and required anyway for
any module that ports their GPL code). The game data is **derived from MegaMek's game data**, so it
carries the same license MegaMek uses for
its data: **Creative Commons Attribution-NonCommercial-ShareAlike 4.0 International**
(<https://creativecommons.org/licenses/by-nc-sa/4.0/>). The released binary **embeds** this data
(`crates/app/src/main.rs`, `include_bytes!`), so the binary as a whole is subject to the
**NonCommercial** and **ShareAlike** terms — Neurohelmet may not be sold or used in a commercial service,
and adaptations of the bundled data must stay under a compatible non-commercial license.

## Data attribution (required by CC-BY-NC-SA-4.0)

The unit catalog, equipment, record-sheet geometry, and faction/era availability that Neurohelmet bakes
are sourced **at build time** from the **MegaMek** ecosystem via **Mekbay**'s data host:

- **MegaMek** — the original compiler of the BattleTech game data. © The MegaMek Team. Code GPLv3,
  data CC-BY-NC-SA-4.0. <https://github.com/MegaMek/megamek>
- **Mekbay** — the data host Neurohelmet downloads from (`db.mekbay.com` / `mekbay.com`): `units.json`,
  `equipment2.json`, `eras.json`, `factions.json`, the unit-availability table, and per-unit
  record-sheet SVGs. Part of the MegaMek family. <https://github.com/MegaMek/mekbay>

**Indication of modifications (per CC-BY-NC-SA-4.0 §3.a):** Neurohelmet does not redistribute the source
data verbatim. It downloads the above, then parses, joins, derives values from, and re-encodes them
into a compact binary bundle (`data/mechs.bin`); it also merges a small set of hand-entered units
(`data/extra_units.json`) for gaps in the source catalog. The bundle is therefore an **adaptation** of
MegaMek/Mekbay data, licensed under CC-BY-NC-SA-4.0.

## BattleTech: Override (Death From Above Wargaming)

**Override** is a streamlined fan ruleset by **Death From Above Wargaming (DFA)**
(<https://dfawargaming.com>). Neurohelmet includes Override support **with DFA's permission**. The mode is
an **independent, non-commercial implementation** of the published ruleset — the card conversion is a
from-scratch Rust port (`crates/core/src/engine/override_conv.rs`), not a copy of DFA's converter. To
drive that conversion the binary embeds DFA's weapon database (`override_weapons.json`,
`weapon_alias.json`). Neurohelmet is not affiliated with or endorsed by DFA. Find out more about
BattleTech: Override and DFA at <https://dfawargaming.com>.

## BattleTech intellectual property

MechWarrior, BattleMech, \`Mech and AeroTech are registered trademarks of The Topps Company, Inc.
All Rights Reserved.

Catalyst Game Labs and the Catalyst Game Labs logo are trademarks of InMediaRes Productions, LLC.

MechWarrior Copyright Microsoft Corporation. Neurohelmet was created under Microsoft's "Game Content Usage
Rules" <https://www.xbox.com/en-US/developers/rules> and it is not endorsed by or affiliated with
Microsoft.

## Third-party software

Neurohelmet's Rust dependencies (e.g. `ratatui`, `serde`, `nucleo-matcher`, `ureq`, `svg2pdf`/`usvg`) are
used under their own licenses; see each crate's license metadata (`cargo tree` / `cargo about`).

**Bundled font.** The PDF record-sheet exporter (`--pdf` / in-app `P`) embeds **Roboto**
(`crates/app/assets/fonts/Roboto-Regular.ttf`, `Roboto-Bold.ttf`) to render sheet text — the same face
Mekbay renders its record sheets with. Roboto is **Apache-2.0** licensed (full text in
`crates/app/assets/fonts/Roboto-LICENSE.txt`).

**Bundled record-sheet logos.** The SBF record sheet reproduces the standard BattleTech and Catalyst
Game Labs logos (`crates/app/assets/logos/BT_Logo_BW.png`, `CGL_Logo.png`), copied from **MegaMek's
assets** (CC-BY-NC-SA-4.0) and drawn exactly as MegaMek's own `SBFRecordSheet` does. These are
trademarks of The Topps Company / InMediaRes Productions (see *BattleTech intellectual property*
above); Neurohelmet follows MegaMek/Mekbay's long-standing practice of reproducing them on fan record
sheets under Microsoft's Game Content Usage Rules.
