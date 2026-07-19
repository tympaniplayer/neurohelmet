# Running on a Raspberry Pi

Neurohelmet is a natural fit for a Pi on the game table: it's a single self-contained binary with
the whole 9,724-unit catalog baked in, it never touches the network at runtime, and it's entirely
keyboard-driven. The default layout profile — and the theme every non-truecolor terminal gets by
default — are literally named **`pi`**: a 16-color palette that respects your terminal's own
background, and a compact layout tuned for a ~100×30 terminal. Plug a Pi into a small screen, and every record sheet for game night is already on it.

Tested target: **Raspberry Pi 4 or 5 running 64-bit Raspberry Pi OS**.

There are two ways to get it on the device: install a prebuilt package from the apt repo (fast,
no compiling), or build it on the Pi itself.

## Route 1: apt (no compiling)

Raspberry Pi OS 64-bit can use the signed apt repository directly — releases ship an arm64 `.deb`:

```sh
sudo mkdir -p /usr/share/keyrings
curl -fsSL https://tympaniplayer.github.io/neurohelmet-apt/neurohelmet.gpg \
  | sudo tee /usr/share/keyrings/neurohelmet.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/neurohelmet.gpg] https://tympaniplayer.github.io/neurohelmet-apt stable main" \
  | sudo tee /etc/apt/sources.list.d/neurohelmet.list
sudo apt update && sudo apt install neurohelmet
```

New releases then arrive with your normal `sudo apt update && sudo apt upgrade`. This is the same
apt channel described on the [Installation](install.md) page; the rest of this page is for
building on-device.

## Route 2: build on the device

You need a C linker and a stable Rust toolchain — that's it. The dataset is committed in the repo
and embedded at build time, so there's no separate data download — the clone brings the whole
catalog with it.

```sh
# 1. One-time: install the C linker + Rust toolchain
sudo apt update && sudo apt install -y build-essential
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# 2. Get the source, then install onto your PATH
git clone https://github.com/tympaniplayer/neurohelmet.git
cd neurohelmet
cargo install --path crates/app       # first build takes a few minutes on a Pi
neurohelmet
```

`cargo install` puts the binary in `~/.cargo/bin` (already on your PATH via rustup), so it
launches as `neurohelmet` from anywhere. If you'd rather not install, `cargo build --release`
works too — the binary lands at `./target/release/neurohelmet`.

If you plan to [publish game logs](guides/game-log.md) from the Pi, also grab the GitHub CLI
while you're at it: `sudo apt install -y gh`. It's not needed for building or playing.

### Memory on a 2 GB Pi

The final link step is the hungry part. If the linker gets killed on a 2 GB Pi, build with fewer
parallel jobs:

```sh
cargo build --release -j 2
```

— or add swap. A 4 GB Pi 4/5 builds comfortably with no tuning.

### Updating

Each install is a snapshot — updating is just a fresh pull and reinstall:

```sh
git pull && cargo install --path crates/app
```

Your sessions are untouched by an update (they live outside the source tree — see below).

## Where your sessions live

On the Pi (as on any Linux box), sessions, logs, and config live under
`~/.local/share/neurohelmet/` — one JSON file per session in `sessions/`. Everything
**autosaves** after every change, and the most recently active session is reopened on launch, so
you can pull the plug between game nights without ceremony. Set `NEUROHELMET_DIR` to relocate the
whole directory. See [Sessions & autosave](guides/sessions.md) and
[Configuration](reference/configuration.md).

## Offline by design

The unit catalog is compiled into the binary, and the app makes no network requests at runtime —
exactly what you want for a Pi in a basement with no Wi-Fi. The only time the network matters is
refreshing the catalog itself: re-bake the data on any machine with internet access (the Pi
included), commit the new `data/mechs.bin`, and rebuild. See [Data & re-baking](reference/data.md).

## Fitting the screen

The compact **Pi** layout profile targets a terminal of roughly **100×30** cells. Not sure what
your display gives you? Run `neurohelmet --term-size` to print what the terminal reports (see the
[Command-line reference](reference/cli.md)). Themes, the roomier Modern layout, and icon sets are
all switchable in-app with **`Ctrl+T`** — the [Themes & layout](guides/display.md) guide covers
the options, including font suggestions for small screens.

Ready to play? Head to [Your first session](first-session.md).
