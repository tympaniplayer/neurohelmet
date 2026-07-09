# Releasing Neurohelmet

Releases are built by [`.github/workflows/release.yml`](../.github/workflows/release.yml).
Everything hangs off one substrate: **a GitHub Release with the compiled binaries.**
The package managers (Homebrew / Scoop / apt) are thin manifests that point at those
release assets by URL + SHA256.

## What gets built

| Target | Artifact | Consumed by |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu` | `.tar.gz` + `.deb` + `.rpm` | Homebrew, apt, AUR, dnf (`.rpm`) |
| `aarch64-unknown-linux-gnu` (Raspberry Pi) | `.tar.gz` + `.deb` + `.rpm` | Homebrew, apt, AUR, dnf (`.rpm`) |
| `x86_64-pc-windows-msvc` | `.zip` | Scoop |
| universal macOS (Intel + Apple Silicon) | `.zip` | Homebrew (macOS) |

The `.rpm`s are attached to each Release for direct `dnf install ./file.rpm` — there's
no hosted RPM repo (yet). The AUR package (`neurohelmet-bin`) is a prebuilt-binary
package that downloads the `.tar.gz`.

No code signing / notarization: the dataset is embedded in the binary, and
`brew`/`scoop`/`apt` don't quarantine their downloads, so unsigned binaries just run.
(A macOS user who downloads the `.zip` *directly from the Releases page in a browser*
will hit Gatekeeper once — they clear it with `xattr -dr com.apple.quarantine neurohelmet`
or right-click → Open. Package-manager installs are unaffected.)

## Cutting a release

```sh
# 1. Bump the version (must match the tag) and commit.
#    Edit [workspace.package] version in Cargo.toml, e.g. 0.1.0 -> 0.2.0
git commit -am "Release v0.2.0"

# 2. Tag and push. The tag push triggers the release workflow.
git tag v0.2.0
git push origin main --tags
```

The workflow guards that the tag (`v0.2.0`) matches the crate version (`0.2.0`) and
fails early if they disagree. To re-run against an existing tag, use the
**Run workflow** button (workflow_dispatch) and enter the tag.

**The publish jobs (Homebrew/Scoop/apt) skip themselves until their secrets exist**,
so you can push a first tag *now* and verify the core GitHub Release works before
setting up any channels.

---

## One-time setup

Everything below is done once. `<owner>` is your GitHub account/org (`tympaniplayer`).

### 1. Create the three channel repos

Package managers require dedicated repos (naming conventions are load-bearing):

| Repo | Purpose | Notes |
| --- | --- | --- |
| `<owner>/homebrew-tap` | Homebrew formula | The `homebrew-` prefix is required for the `brew install <owner>/tap/...` shorthand. |
| `<owner>/scoop-bucket` | Scoop manifest | Any name works; `scoop-bucket` is conventional. |
| `<owner>/neurohelmet-apt` | apt repo (files served via Pages) | Enable **Settings → Pages → Deploy from a branch → `main` / root** after the first push. |

Create them empty (a bare README is fine). CI populates them.

### 2. Deploy token — `PACKAGES_DEPLOY_TOKEN`

The workflow runs in the `neurohelmet` repo but has to push to the three repos above,
which the default `GITHUB_TOKEN` can't reach. Create a **fine-grained personal access
token**:

- GitHub → **Settings → Developer settings → Fine-grained tokens → Generate new token**
- **Repository access:** only the three channel repos.
- **Permissions:** *Contents: Read and write*.
- Copy the token, then add it in **`neurohelmet` → Settings → Secrets and variables →
  Actions → New repository secret** as `PACKAGES_DEPLOY_TOKEN`.

Setting this one secret enables the Homebrew **and** Scoop jobs.

### 3. apt signing key — `APT_GPG_PRIVATE_KEY`

The apt repo's metadata must be GPG-signed. Generate a dedicated **passphrase-less**
key (it lives only as a GitHub secret; no passphrase keeps CI non-interactive):

```sh
gpg --batch --passphrase '' \
  --quick-generate-key "Neurohelmet Apt Repo <nate@natedpalm.com>" rsa4096 sign never

# Find the key id and export the PRIVATE key (armored):
gpg --list-secret-keys --keyid-format=long
gpg --armor --export-secret-keys <KEYID> > apt-private.asc
```

Add the contents of `apt-private.asc` as the secret `APT_GPG_PRIVATE_KEY` in the
`neurohelmet` repo. The workflow imports it, derives the key id automatically, signs
the repo, and publishes the matching **public** key (`neurohelmet.gpg`) into the apt
repo for users. Delete `apt-private.asc` afterward.

Setting `APT_GPG_PRIVATE_KEY` **and** `PACKAGES_DEPLOY_TOKEN` enables the apt job.

### 4. AUR deploy key — `AUR_SSH_PRIVATE_KEY`

The AUR lives on `aur.archlinux.org` (not GitHub) and authenticates over SSH. The
CI deploy key is already generated and stored as the secret `AUR_SSH_PRIVATE_KEY`; you
just need to register its **public** half against a free AUR account:

1. Create an account at [aur.archlinux.org/register](https://aur.archlinux.org/register).
2. **My Account → SSH Public Key** → paste the public key (the one printed when the key
   was generated; re-print any time with `ssh-keygen -y -f <path-to-private-key>`), save.

That's it — no need to pre-create the package. The first release pushes to
`ssh://aur@aur.archlinux.org/neurohelmet-bin.git`, which the AUR auto-creates. RPMs need
**no** setup: they're built by `cargo generate-rpm` and attached to every Release.

> The account must own the `neurohelmet-bin` name. If someone else already published it,
> pick a different `pkgname` in `packaging/aur/PKGBUILD` + `.SRCINFO`.

### Secret checklist

| Secret | Enables | Where to get it |
| --- | --- | --- |
| `PACKAGES_DEPLOY_TOKEN` | Homebrew + Scoop + apt push | Fine-grained PAT (step 2) |
| `APT_GPG_PRIVATE_KEY` | apt repo signing | `gpg --export-secret-keys` (step 3) |
| `AUR_SSH_PRIVATE_KEY` | AUR push | ed25519 deploy key; public half on your AUR account (step 4) |

That's it — no Apple Developer account, no certificates.

---

## How users install

**Homebrew (macOS + Linux)**
```sh
brew install <owner>/tap/neurohelmet
```

**Scoop (Windows)**
```powershell
scoop bucket add neurohelmet https://github.com/<owner>/scoop-bucket
scoop install neurohelmet
```

**apt (Debian/Ubuntu, incl. Raspberry Pi OS 64-bit)**
```sh
sudo mkdir -p /usr/share/keyrings
curl -fsSL https://<owner>.github.io/neurohelmet-apt/neurohelmet.gpg \
  | sudo tee /usr/share/keyrings/neurohelmet.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/neurohelmet.gpg] https://<owner>.github.io/neurohelmet-apt stable main" \
  | sudo tee /etc/apt/sources.list.d/neurohelmet.list
sudo apt update && sudo apt install neurohelmet
```

**AUR (Arch Linux)**
```sh
yay -S neurohelmet-bin
```

**Fedora / RHEL / openSUSE (.rpm)**
```sh
sudo dnf install ./neurohelmet-<version>-x86_64-unknown-linux-gnu.rpm
```

**Direct download** — grab an archive from the
[Releases page](https://github.com/<owner>/neurohelmet/releases); verify against
`SHA256SUMS`.
