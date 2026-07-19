# Game log & publishing

Neurohelmet can keep a **turn-by-turn log** of a session and turn it into shareable images — the
offline analog to writing up a battle report. This page covers capturing a log, exporting it
locally, and publishing it to the web.

## Capturing a log

While tracking a game, press **`L`** to snapshot the current state. Each press appends one entry to
an append-only log file next to the session — a session you never snapshot has no log at all. The
status line confirms with `Logged Turn N (X mechs)`, and once you've logged at least once the
session tab bar shows a ` · log N` counter. Snapshotting an empty roster does nothing
(`Nothing to log (empty roster)`).

A snapshot records the **complete state of every tracked unit** — damage, heat, criticals, ammo,
pilots — plus which game system the session was in, and for Strategic BattleForce and BattleForce
the formation/Unit grouping and round state too. Each entry embeds its own unit specs, so old logs
re-render faithfully even after the data bundle changes.

"Turn" here just means *snapshot count* — Neurohelmet imposes no turn or phase structure. Press
**`L`** whenever you want a frame in the story: end of every round, or just at the dramatic
moments. The counter persists with the session, so numbering continues across restarts.

### Which modes can log

**`L`** captures snapshots in **Classic, Alpha Strike, Override, Strategic BattleForce, and
BattleForce**. The **Abstract Combat System has no game log** — in ACS, `L` is part of the
aerospace shot editor instead. The printable artifact for an ACS session is its
[PDF record sheet](pdf-record-sheets.md).

> **Edge case**: `z` undo rolls back the turn counter after an `L`, but it can't un-append the
> already-written log line — so the next `L` will record a duplicate turn number. Harmless, but
> worth knowing if you undo right after logging.

## Exporting locally (no git, fully offline)

```sh
neurohelmet --export <session> [outdir]
```

Renders every logged turn to image frames plus a text transcript. This touches nothing but your
disk — no accounts, no network, and it doesn't even need a terminal or the data bundle. If you
just want images to convert and drop into a chat or forum post, stop here.

By default the export lands in `<data-dir>/sessions/<name>-log/`; pass a second argument to choose
a different output directory. For each turn you get:

- `turn-NN/` — one **PPM** image per frame
- `turn-NN.ppm` — all of that turn's frames stacked into a single montage

plus a single `transcript.txt` for the whole log — a plain-text render of every frame, each headed
by its turn label.

What counts as a "frame" depends on the mode the snapshot was taken in: Classic, Alpha Strike, and
Override logs get **one frame per unit**, each re-rendered in its own mode — an Override snapshot
exports the Override doll, an Alpha Strike snapshot the card. SBF logs get one frame per
**formation** (the formation sheet), and BattleForce logs one frame per **Unit**. Frames render
exactly like the live screen on a 100×30 terminal — the same view you see in
[the tracker](../modes/classic.md) — always in the Pi theme and layout, regardless of your own
display settings.

Note the format: `--export` writes **PPM** files (a simple raw format most image viewers and
converters read). PNGs are what `--publish` produces.

## Publishing to the web

```sh
neurohelmet --publish <session> [--dry-run]
```

`--publish` renders the same frames and montages as **PNG** — the format GitHub renders inline —
and commits them into a git repo as a browsable gallery. `--dry-run` writes the gallery to a local
staging directory (`logs-staging/` in your data dir) and skips all git and network work, so you
can preview exactly what would be published. Unlike `--export`, publishing requires a non-empty
log — it stops with an error if you never pressed `L`.

The gallery is plain markdown plus images:

```text
README.md                # landing page (written once; your edits survive)
games/
  <session-name>/
    README.md            # the gallery page — newest turn first
    turn-01.png          # montage for turn 1
    turn-01/             # individual frames
      01-Atlas AS7-D.png
      …
```

There are two ways to publish.

### Option A — zero config (GitHub CLI)

The default. With the [GitHub CLI](https://cli.github.com/) installed and authenticated:

```sh
gh auth login          # one time
neurohelmet --publish my-game
```

Neurohelmet creates a public `neurohelmet-logs` repo under your account (once), keeps a working
clone in your data dir, writes the gallery, pushes, enables GitHub Pages, and prints the gallery
URL — `https://<you>.github.io/neurohelmet-logs/games/<session>/`. Auth rides on your existing
`gh` login; there's nothing to configure.

### Option B — bring your own repo (no GitHub CLI)

Don't use `gh`, or want GitLab / a self-hosted remote / an existing repo? Set **`log_repo`** in
your [config](../reference/configuration.md) to a **local git working copy**. Neurohelmet writes
the gallery into that repo and skips all `gh` plumbing.

```sh
git clone git@github.com:you/my-battle-logs.git ~/battle-logs
```

```json
{ "log_repo": "~/battle-logs" }
```

Now `neurohelmet --publish my-game` writes into `~/battle-logs`, commits
(`my-game: N turn(s)`), and pushes to that clone's remote. A leading `~/` expands to your home
directory. Neurohelmet only ever stages the paths it owns (`games/` and the root `README.md`), so
it's safe to point `log_repo` at a repo that also holds unrelated files. The repo must already
exist — if the path has no `.git`, publish stops and asks you to `git clone` or `git init` it
first.

### Manual pushes — `log_auto_push`

By default publishing pushes automatically. To push on your own schedule, set:

```json
{ "log_auto_push": false }
```

With auto-push off, `--publish` renders and **commits**, then stops. It prints the repo path so
you can push when you're ready (`git -C <repo> push`). This works for both options above.

See [Configuration](../reference/configuration.md) for the full list of config keys and where the
config file lives, and [Sessions & autosave](sessions.md) for where the log file itself sits on
disk.
