# Data & re-baking

Neurohelmet ships with its entire unit catalog **baked into the binary**. There is no download on
first launch, no account, and no network traffic at runtime — the bundle is compiled in from
`data/mechs.bin`, so the app works the same on a plane, at a con with no Wi-Fi, or on a
[Raspberry Pi](../raspberry-pi.md) in a basement. Most people never need this page: the shipped
bundle is complete. Read on if you're curious where the data comes from, or you want to re-bake it
yourself.

## Where the data comes from

The bake pipeline pulls from **[Mekbay](https://mekbay.com)** (`db.mekbay.com`), whose data is in
turn derived from the **MegaMek** project's unit files. The baker downloads:

- `units.json` — the master unit catalog,
- `equipment2.json` — weapons and equipment,
- the availability catalogs — the era and faction lists, plus a rarity table fetched from
  `mekbay.com` itself — the data behind the picker's
  [availability lens](../guides/force-generation.md),
- each unit's **record-sheet SVG**, parsed for per-unit detail.

Everything lands in a local cache, so subsequent bakes only pay the network cost once.

A handful of hand-entered extras from `data/extra_units.json` (gun emplacements and Battlefield
Support Alpha Strike cards) are merged in before the bundle is written.

> **Licensing.** The unit data is CC-BY-NC-SA, via MegaMek and Mekbay. That's why release
> artifacts are non-commercial — see [License & attribution](attribution.md) for the full picture.

## What's in the bundle

| | |
|---|---|
| Units | **9,724** — 'Mechs, combat vehicles, infantry & battle armor, aerospace fighters, and large craft (DropShips, Small Craft, JumpShips, WarShips, Space Stations). Only ProtoMechs are excluded. |
| Per unit | Per-location armor, heat sinks, weapons with to-hit numbers, ammo bins with munition options, equipment, Alpha Strike stats, and design quirks (on ~3,300 units). |
| Also carried | The munition catalog, era table, and faction table that power the picker's availability lens. |
| Size | 11,044,780 bytes (~11 MB), bundle format **v24**. |

You can confirm what your build loaded with `neurohelmet --selftest` — its first line is
`loaded 9724 mechs`.

## Re-baking

The baker is a separate crate in the repo, `neurohelmet-bake`. A full re-bake, straight from the
README:

```sh
cargo run --release -p neurohelmet-bake -- --jobs 4 --out data/mechs.bin
```

| Flag | Default | What it does |
|------|---------|--------------|
| `--out <path>` | `data/mechs.bin` | Output bundle path. **Required explicitly** for filtered or limited bakes — see the guard below. |
| `--cache <dir>` | `.bake-cache` | Download cache (gitignored). Re-bakes reuse it, so only the first run hits the network hard. |
| `--jobs N` | | Thread count for the parallel SVG fetch/parse pass. |
| `--filter SUBSTR` | | Case-insensitive substring match on "chassis model" — bake only matching units. |
| `--limit N` | | Truncate to the first N units after filtering. |
| `--print` | | Dump every baked unit's stats (heat sinks, per-location armor, weapons with to-hit, ammo bins with munition options, equipment) to stdout. |

Unknown flags exit with code 2. When it finishes, the baker reports skipped units (first 25 shown)
and ends with `wrote <path> (X.X MB)`.

**Be gentle with `--jobs`.** Mekbay is a hobbyist-run host. The fetcher already backs off on
rate-limit and server errors (exponential retry, up to 7 attempts, 750 ms doubling to a 30 s cap),
but a polite job count is still the right default.

### The partial-bake guard

A bake with `--filter` or `--limit` produces a **partial bundle** — fine for testing, disastrous
if it silently replaces the real one. So the baker refuses to write a filtered/limited subset
without an explicit `--out`:

```text
refusing to overwrite data/mechs.bin with a filtered/limited subset (N units).
pass an explicit --out <path> (e.g. --out /tmp/subset.bin) for a partial bake.
```

Keep partial bakes out of `data/mechs.bin`: only a full, unfiltered bake belongs there.

### After a full re-bake

The bundle is embedded at compile time, so **rebuild the app** after re-baking — a new
`data/mechs.bin` on disk changes nothing until the binary is recompiled.

To try a bundle *without* rebuilding, point the **`NEUROHELMET_DATA`** environment variable at it:

```sh
NEUROHELMET_DATA=/tmp/subset.bin neurohelmet --selftest
```

The app loads that file instead of the embedded copy — handy for checking a test bake before
committing to it. See [Configuration](configuration.md) and the
[command-line reference](cli.md) for the other environment variables.

### Your sessions survive

Saved sessions store each unit's baked spec, and on load every tracked unit is **re-linked to the
current bundle** — so a re-bake (or an app upgrade with a newer bundle) doesn't strand your
in-progress games. See [Sessions & autosave](../guides/sessions.md).
