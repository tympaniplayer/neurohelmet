# Template for the Homebrew formula. On each tagged release,
# .github/workflows/release.yml fills in the __PLACEHOLDERS__ (version + SHA256s)
# and pushes the result to <owner>/homebrew-tap as Formula/neurohelmet.rb.
# Don't edit the copy in the tap by hand — it's overwritten every release.
class Neurohelmet < Formula
  desc "Keyboard-driven terminal BattleTech record-sheet tracker"
  homepage "https://github.com/__OWNER__/neurohelmet"
  version "__VERSION__"
  license "GPL-3.0-or-later"

  on_macos do
    url "https://github.com/__OWNER__/neurohelmet/releases/download/v__VERSION__/neurohelmet-__VERSION__-universal-apple-darwin.zip"
    sha256 "__SHA_MACOS__"
  end

  on_linux do
    on_intel do
      url "https://github.com/__OWNER__/neurohelmet/releases/download/v__VERSION__/neurohelmet-__VERSION__-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "__SHA_LINUX_X86__"
    end
    on_arm do
      url "https://github.com/__OWNER__/neurohelmet/releases/download/v__VERSION__/neurohelmet-__VERSION__-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "__SHA_LINUX_ARM__"
    end
  end

  def install
    bin.install "neurohelmet"
  end

  test do
    assert_match "loaded", shell_output("#{bin}/neurohelmet --selftest")
  end
end
