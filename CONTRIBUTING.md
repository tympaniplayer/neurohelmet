# Contributing to Neurohelmet

Thanks for taking an interest. Neurohelmet is a hobby project for playing BattleTech at a real table,
and contributions of all sizes are welcome — bug reports, a fix for a rules edge case, a new theme,
or a whole game system.

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Before you build something big

Neurohelmet has three design invariants. They're what makes it feel the way it does, and a change that
breaks one is unlikely to land no matter how well it's written:

- **Offline and self-contained.** The unit catalog is baked into the binary. The app never touches
  the network at runtime.
- **Manual first.** You roll the dice. Neurohelmet does the bookkeeping and surfaces the consequences —
  it never rolls for you, and it never hides a decision. Automation is opt-in and warns before it
  discards anything you entered by hand.
- **One screen.** The layout targets a ~100×30 terminal (a 7" Raspberry Pi display).

[ROADMAP.md](ROADMAP.md) records the planned work, the **non-goals**, and the known limitations —
worth a read before starting something substantial. For anything large, open an issue first so we can
talk about the shape of it before you spend a weekend on it.

## Getting set up

You need a recent stable Rust toolchain ([rustup](https://rustup.rs)). Then:

```sh
cargo build --release            # builds all crates (embeds data/mechs.bin)
cargo run -p neurohelmet         # run the tracker (needs a real terminal)
cargo run -p neurohelmet -- --selftest   # headless: render one frame, no terminal needed
```

`--selftest` is the quickest way to confirm a build works in an environment without a TTY.

The workspace:

```
crates/core   domain types, rules engines, session/persistence, bundle loader — no UI
crates/bake   downloads Mekbay data and bakes it into data/mechs.bin
crates/app    the ratatui app (binary: `neurohelmet`)
```

`core` has no terminal dependencies and all its rules are pure and unit-tested; ratatui stays inside
`crates/app`. Keeping that boundary makes the rules testable, so please don't reach across it.

## Making a change

Work on a branch and open a pull request — `main` is protected, so it takes PRs only.

**Format before you push.** CI fails any PR that isn't `cargo fmt`-clean:

```sh
cargo fmt --all
```

**Run the tests.** `cargo test` covers three layers:

- **Rules and data** (`core`, `bake`) — damage cascade, heat, ammo, crits, session persistence,
  SVG-parse fixtures, and golden-card conversions for Override and the BattleForce family (checked
  against MegaMek's output).
- **Interaction** (`app`) — real `KeyEvent`s driven through `handle_key`, asserting on state.
- **Snapshots** (`app`) — a key sequence, a full 100×30 frame rendered with ratatui's `TestBackend`,
  diffed against committed `.snap` files. These are readable pictures of every screen and they catch
  a surprising amount.

After an intentional UI change the snapshots will fail. Regenerate and *read* the diff before
committing — it's the review:

```sh
INSTA_UPDATE=always cargo test -p neurohelmet
git diff crates/app/src/tui/snapshots/
```

Tests that touch the disk must call `isolate_data_dir()` first, so they never write to a real user's
sessions directory.

## Keybindings live in four places

If you add or change a key, all four need to agree, and they've drifted before:

1. The key dispatcher in `crates/app/src/tui/app.rs` (the actual behavior).
2. The in-app `?` help modal for that mode, in `crates/app/src/tui/view.rs`.
3. The status-bar footer hint for that screen, also in `view.rs`.
4. `docs/keybindings-cheatsheet.html` — and its committed PDF, which must be re-rendered:

```sh
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless --disable-gpu --no-pdf-header-footer \
  --print-to-pdf="docs/neurohelmet-keybindings.pdf" \
  "file://$(pwd)/docs/keybindings-cheatsheet.html"
```

The guide's [keybindings reference](docs/guide/src/reference/keybindings.md) is a fifth place worth
updating for anything user-visible.

## Documentation

The guide is an [mdBook](https://rust-lang.github.io/mdBook/) under `docs/guide` — source in
`docs/guide/src`, published to GitHub Pages automatically when a change to it lands on `main`.

```sh
mdbook serve docs/guide     # live preview at http://localhost:3000
```

**Screenshots are generated, not captured by hand.** `crates/app/src/tui/screenshots.rs` renders
curated scenes through the same rasterizer as `--export`. After a UI change that alters a documented
screen, regenerate them and eyeball the result:

```sh
cargo test -p neurohelmet --release docs_screenshots -- --ignored --nocapture
```

## Game data

`data/mechs.bin` is committed and embedded at build time. You only need to re-bake it when the
upstream data changes:

```sh
cargo run --release -p neurohelmet-bake -- --jobs 4 --out data/mechs.bin
```

Be gentle with `--jobs` — `db.mekbay.com` rate-limits with HTTP 429. Downloads cache under
`.bake-cache/`, so re-bakes are cheap.

**A filtered bake produces a partial bundle.** If you use `--filter` or `--limit`, don't commit the
result — check the file size (a full bundle is ~11 MB, 9,724 units) before committing any change that
touches it. Rebuild the app afterward so the new bundle is embedded.

To test a bundle without rebuilding, point the app at it: `NEUROHELMET_DATA=/path/to/mechs.bin`.

## Reporting bugs

Open an issue with what you did, what you expected, and what happened, plus your version
(`neurohelmet --version`) and OS. For anything visual, the terminal, font, and `NEUROHELMET_THEME` /
`NEUROHELMET_PROFILE` / `NEUROHELMET_ICONS` settings are useful — and a paste of the screen is worth a
lot, since it's all text.

Found a **security** issue? Report it privately instead — see [SECURITY.md](SECURITY.md).

Rules bugs are the most valuable kind: if Neurohelmet disagrees with the book, cite the rulebook and
page and it'll get fixed.

## Licensing

Neurohelmet's code is **GPL-3.0-or-later**, and the bundled game data is MegaMek-derived and
**CC-BY-NC-SA-4.0** — which makes the binary as a whole non-commercial. By contributing, you agree
your contribution ships under those terms. See [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md) for the
full picture, including the Override ruleset's attribution to Death From Above Wargaming.
