# The Homebrew formula for the Space Lens command-line interface (`spacelens`).
#
# This file lives in the Space Lens repository as the source of truth; each
# release, fill `version` and the sha256 values from the real release
# artifacts (the `cli-v*` tag job in cli-release.yml publishes the tarballs), then
# push this file to `HuakunShen/homebrew-tap` as `Formula/spacelens.rb` (the
# push is the owner's action — this repository never pushes).
#
# Fill the sha256 values with:
#   shasum -a 256 <(curl -sL <tarball url>)
# then verify locally with:
#   brew audit HuakunShen/homebrew-tap/spacelens
#   brew install HuakunShen/homebrew-tap/spacelens
#
# These are the same prebuilt archives `cargo binstall spacelens` downloads;
# the layout is produced by .github/workflows/cli-release.yml.

class Spacelens < Formula
  desc "Scan disk usage and find cleanup candidates for development projects"
  homepage "https://github.com/HuakunShen/space-lens"
  version "0.3.1"
  license "MIT"

  livecheck do
    # Homebrew 7: the crates_io strategy became :crate and expects the
    # static.crates.io download URL; it derives the versions API itself.
    url "https://static.crates.io/crates/spacelens/spacelens-#{version}.crate"
    strategy :crate
  end

  # `spacelens dirty-git` shells out to git for `git status --porcelain`.
  uses_from_macos "git"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/HuakunShen/space-lens/releases/download/cli-v#{version}/spacelens-aarch64-apple-darwin-v#{version}.tar.gz"
      sha256 "cb75016cfef1e4b88df0375e2ec46a9deb0f8daabc9d0d0e380b0b2377204ead"
    else
      url "https://github.com/HuakunShen/space-lens/releases/download/cli-v#{version}/spacelens-x86_64-apple-darwin-v#{version}.tar.gz"
      sha256 "c7e1091a4b9d3303070f2f7bc9edbcf650aa32f73d69b39d5f3c976d734addd9"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/HuakunShen/space-lens/releases/download/cli-v#{version}/spacelens-aarch64-unknown-linux-musl-v#{version}.tar.gz"
      sha256 "94b027b1be790d5e6380fa48393cb34d74da3f8bf3a021435b53d208004ba29d"
    else
      url "https://github.com/HuakunShen/space-lens/releases/download/cli-v#{version}/spacelens-x86_64-unknown-linux-musl-v#{version}.tar.gz"
      sha256 "0fc37a2bb92facdbf9a250fb4ed1404acb82be8a5ce2610084e7d65090b299f1"
    end
  end

  def install
    arch = Hardware::CPU.intel? ? "x86_64" : "aarch64"
    os = OS.mac? ? "apple-darwin" : "unknown-linux-musl"
    # Homebrew strips the archive's single root directory when unpacking;
    # accept both layouts so older brews keep working.
    staged = "spacelens-#{arch}-#{os}-v#{version}/spacelens"
    bin.install File.exist?(staged) ? staged : "spacelens"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/spacelens --version")
    assert_match "dry run", shell_output("#{bin}/spacelens clean #{testpath}")
  end
end
