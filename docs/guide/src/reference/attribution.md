# License & attribution

Neurohelmet is an **unofficial, non-commercial, fan-made** BattleTech play aid. It is **not** affiliated
with, endorsed by, or licensed by Microsoft, The Topps Company, Catalyst Game Labs, Death From Above
Wargaming, or the MegaMek project.

This page is a friendly summary; the authoritative texts are
[`LICENSE`](https://github.com/tympaniplayer/neurohelmet/blob/main/LICENSE) and
[`NOTICE.md`](https://github.com/tympaniplayer/neurohelmet/blob/main/NOTICE.md) in the repository.

## Licensing at a glance

| Part | License |
|------|---------|
| Source code | **GPL-3.0-or-later** |
| Bundled game data (`data/mechs.bin`, embedded in the binary) | **CC-BY-NC-SA-4.0** |
| Bundled Roboto font (embedded for PDF record sheets) | **Apache-2.0** |
| Bundled record-sheet logos (from MegaMek's assets) | **CC-BY-NC-SA-4.0** |

The code is GPLv3-or-later, matching the MegaMek ecosystem. The bundled game data is **derived from
MegaMek data** and licensed CC-BY-NC-SA-4.0. Because the binary embeds that data, the binary as a
whole is **non-commercial** — Neurohelmet may not be sold or used in a commercial service, and
adaptations of the bundled data must stay under a compatible non-commercial license.

## Game data — MegaMek & Mekbay

The unit catalog, equipment, record-sheet geometry, and faction/era availability are sourced at build
time from the **MegaMek** ecosystem via **Mekbay**'s data host, then parsed, derived, and re-encoded
into Neurohelmet's bundle (an adaptation, per CC-BY-NC-SA-4.0). See
[Data & re-baking](data.md) for what the bundle contains and how it's built.

- **[MegaMek](https://github.com/MegaMek/megamek)** — the original compiler of the BattleTech game data.
- **[Mekbay](https://github.com/MegaMek/mekbay)** — the data host Neurohelmet builds from.

## BattleTech: Override — Death From Above Wargaming

The **[Override](../modes/override.md)** mode is included **with permission from
[Death From Above Wargaming](https://dfawargaming.com)**. Neurohelmet's Override support is an independent,
non-commercial implementation of DFA's published ruleset. Find out more about BattleTech: Override and
DFA at **[dfawargaming.com](https://dfawargaming.com)**.

## PDF record sheets

The exported [PDF record sheets](../guides/pdf-record-sheets.md) reproduce the standard **BattleTech**
and **Catalyst Game Labs** logos, copied from MegaMek's CC-BY-NC-SA asset set and drawn exactly as
MegaMek's own record sheets do, and every page carries the verbatim Topps/CGL record-sheet notice
("Permission to photocopy for personal use") exactly as MegaMek prints it. The logos are trademarks of
The Topps Company / InMediaRes Productions; Neurohelmet follows MegaMek and Mekbay's long-standing
practice of reproducing them on fan record sheets. Sheet text is set in **Roboto** (Apache-2.0), the
face Mekbay renders its sheets with.

## BattleTech intellectual property

MechWarrior, BattleMech, \`Mech and AeroTech are registered trademarks of The Topps Company, Inc.
Catalyst Game Labs and the Catalyst Game Labs logo are trademarks of InMediaRes Productions, LLC.
MechWarrior © Microsoft Corporation; Neurohelmet was created under Microsoft's
"[Game Content Usage Rules](https://www.xbox.com/en-US/developers/rules)" and is not endorsed by or
affiliated with Microsoft.
