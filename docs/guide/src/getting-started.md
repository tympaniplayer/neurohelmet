# Install & first run

## Requirements

- A terminal at least ~100×30 cells (bigger is fine; the layout adapts).
- To build from source: a recent **Rust** toolchain (`rustup`, stable).

## Build & run

```sh
git clone https://github.com/tympaniplayer/neurohelmet.git
cd neurohelmet
cargo run --release -p neurohelmet
```

The catalog is baked into the binary, so there's nothing else to download and no network access at
runtime.

## Your first session

Neurohelmet opens on the **Sessions browser**. Each session is locked to one game system when you create
it, and you can keep many named sessions side by side.

Press the key for the system you want:

| Key | New session |
|-----|-------------|
| `n` | Classic (Total Warfare) |
| `A` | Alpha Strike |
| `O` | Override |
| `B` | Strategic BattleForce |
| `F` | BattleForce |
| `C` | Abstract Combat System |

Type a name, then you're in. From there:

- **`a`** — add a unit (opens the catalog picker; type to filter).
- **`?`** — show the keybindings for the current mode (they differ per system).
- **`L`** — snapshot the current turn to the [game log](guides/game-log.md).
- **`S`** — back to the Sessions browser.
- **`z`** — undo (50 levels deep).
- **`Ctrl-T`** — pick a [theme, layout, and icon set](guides/display.md).
- **`q`** — quit (it asks first).

Everything **autosaves** after every change, and the most recently active session is reopened next
launch.

## Where your data lives

Sessions, logs, and config live under a per-OS data directory (e.g.
`~/.local/share/neurohelmet` on Linux, `~/Library/Application Support/neurohelmet` on macOS). Set the
`NEUROHELMET_DIR` environment variable to relocate all of it together — handy for testing or keeping
separate profiles. See [Configuration](reference/configuration.md) for details.
