# Installation

Neurohelmet is a single self-contained binary. The full unit catalog — 9,724 units, roughly 11 MB —
is baked into the executable at build time, so there is nothing else to download, no data files to
place, and no network access at runtime. Install it, run `neurohelmet`, and you're at the table.

## Requirements

- A terminal at least **~100×30 cells** (bigger is fine — on wide terminals you can switch to a
  [roomier layout profile](guides/display.md)).
- Any decent **monospace font** — the app draws with box-drawing and block-shading characters that
  every standard mono font carries. A Nerd Font is optional but unlocks richer glyphs; see
  [Themes & layout](guides/display.md).
- To build from source: a stable **Rust** toolchain via `rustup`. No minimum version is pinned —
  current stable works.

## Package managers

### Homebrew — macOS · Linux

```sh
brew install tympaniplayer/tap/neurohelmet
```

Covers macOS (universal binary) plus Linux on x86_64 and arm64.

### Scoop — Windows

```powershell
scoop bucket add neurohelmet https://github.com/tympaniplayer/scoop-bucket
scoop install neurohelmet
```

64-bit Windows only.

### apt — Debian · Ubuntu · Raspberry Pi OS (64-bit)

```sh
sudo mkdir -p /usr/share/keyrings
curl -fsSL https://tympaniplayer.github.io/neurohelmet-apt/neurohelmet.gpg \
  | sudo tee /usr/share/keyrings/neurohelmet.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/neurohelmet.gpg] https://tympaniplayer.github.io/neurohelmet-apt stable main" \
  | sudo tee /etc/apt/sources.list.d/neurohelmet.list
sudo apt update && sudo apt install neurohelmet
```

The repository is GPG-signed and serves amd64 and arm64 packages — the arm64 `.deb` is the
no-compile path for a Raspberry Pi. Future `apt upgrade` runs pick up new releases automatically.

### AUR — Arch Linux

```sh
yay -S neurohelmet-bin      # or any AUR helper, e.g. paru -S neurohelmet-bin
```

`neurohelmet-bin` installs the prebuilt release binary (x86_64 and aarch64) — no compilation.

### RPM — Fedora · RHEL · openSUSE

There is no hosted RPM repository; download the `.rpm` for your architecture from the
[latest release](https://github.com/tympaniplayer/neurohelmet/releases) and install the file
directly:

```sh
sudo dnf install ./neurohelmet-*-x86_64-unknown-linux-gnu.rpm
```

An aarch64 RPM is published too.

## Direct download

Every release ships prebuilt archives on the
[GitHub Releases page](https://github.com/tympaniplayer/neurohelmet/releases), each containing the
executable plus README, LICENSE, and NOTICE:

| Platform | Artifact |
|----------|----------|
| Linux x86_64 (glibc 2.35+, e.g. Ubuntu 22.04 or newer) | `neurohelmet-<version>-x86_64-unknown-linux-gnu.tar.gz` (also `.deb`, `.rpm`) |
| Linux arm64 | `neurohelmet-<version>-aarch64-unknown-linux-gnu.tar.gz` (also `.deb`, `.rpm`) |
| Windows x86_64 | `neurohelmet-<version>-x86_64-pc-windows-msvc.zip` |
| macOS (universal: Intel + Apple Silicon) | `neurohelmet-<version>-universal-apple-darwin.zip` |

A `SHA256SUMS` file covering every asset is attached to each release — verify your download with
`sha256sum -c` (or `shasum -a 256` on macOS) before running it.

**macOS Gatekeeper**: the binary is unsigned, and a zip downloaded in a browser gets quarantined.
Clear it once:

```sh
xattr -dr com.apple.quarantine neurohelmet
```

(or right-click the binary → Open, once). Installs via Homebrew, Scoop, or apt are not quarantined.

## Build from source

```sh
git clone https://github.com/tympaniplayer/neurohelmet.git
cd neurohelmet
cargo build --release           # embeds data/mechs.bin into the binary
cargo run -p neurohelmet        # run the tracker (needs a real terminal)
```

The dataset is committed in the repo and embedded at build time, so a source build needs no network
and no extra data step. For building directly on a Raspberry Pi — including the memory-constrained
2 GB case — see [Running on a Raspberry Pi](raspberry-pi.md).

## Verify it runs

```sh
neurohelmet --selftest
```

This is a headless smoke test: no terminal UI, no interaction. It loads the bundle, prints
`loaded 9724 mechs`, and renders one demo frame (a battle-worn Atlas) to stdout. If you see that,
the install is good. There's also `neurohelmet --term-size` to check what size the app thinks your
terminal is — handy if the layout looks cramped. The full verb list lives in the
[command-line reference](reference/cli.md).

## Next step

Run `neurohelmet` with no arguments and head to [Your first session](first-session.md) — you'll be
tracking damage on an Atlas inside ten minutes. If anything misbehaves, check
[Troubleshooting](reference/troubleshooting.md).
