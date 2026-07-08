# Game log & publishing

Neurohelmet can keep a **turn-by-turn log** of a session and turn it into shareable images — the offline
analog to writing up a battle report. This page covers capturing a log, exporting it locally, and
publishing it to the web.

## Capturing a log

While tracking a game, press **`L`** to snapshot the current turn. Each snapshot appends to an
append-only file next to the session; a session you never snapshot has no log at all. Snapshots record
the whole board state (units, damage, heat, criticals, pilots) for every supported mode.

## Exporting locally (no git, fully offline)

```sh
neurohelmet --export <session> [outdir]
```

Renders every logged turn to image frames plus a text transcript. This touches nothing but your disk —
no accounts, no network. If you just want images to drop into a chat or forum post, stop here.

## Publishing to the web

```sh
neurohelmet --publish <session> [--dry-run]
```

`--publish` renders the frames and commits them into a git repo as a browsable gallery. `--dry-run`
writes the gallery to a local staging dir and skips all git/network work, so you can preview exactly
what would be published.

There are two ways to publish.

### Option A — zero config (GitHub CLI)

The default. With the [GitHub CLI](https://cli.github.com/) installed and authenticated:

```sh
gh auth login          # one time
neurohelmet --publish my-game
```

Neurohelmet creates a public `neurohelmet-logs` repo under your account (once), writes the gallery, pushes,
enables GitHub Pages, and prints the URL. Auth rides on your existing `gh` login — nothing to
configure.

### Option B — bring your own repo (no GitHub CLI)

Don't use `gh`, or want GitLab / a self-hosted remote / an existing repo? Set **`log_repo`** in your
[config](../reference/configuration.md) to a **local git working copy**. Neurohelmet writes the gallery
into that repo and skips all `gh` plumbing.

```sh
git clone git@github.com:you/my-battle-logs.git ~/battle-logs
```

```json
{ "log_repo": "~/battle-logs" }
```

Now `neurohelmet --publish my-game` writes into `~/battle-logs`, commits, and pushes to that clone's
remote. A leading `~/` expands to your home directory. Neurohelmet only ever stages the paths it owns
(`games/` and the root `README.md`), so it's safe to point `log_repo` at a repo that also holds
unrelated files. The repo must already exist — if the path has no `.git`, publish stops and asks you
to `git clone` or `git init` it first.

### Manual pushes — `log_auto_push`

By default publishing pushes automatically. To push on your own schedule, set:

```json
{ "log_auto_push": false }
```

With auto-push off, `--publish` renders and **commits**, then stops. It prints the repo path so you can
push when you're ready (`git -C <repo> push`). This works for both options above.

See [Configuration](../reference/configuration.md) for the full list of config keys and where the file
lives.
