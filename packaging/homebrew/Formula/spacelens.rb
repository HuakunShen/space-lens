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
  version "0.4.2"
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
      sha256 "614b241277973b595e04ea899fd9a2015fe71636a49fc90ff3596b21d23d5d8e"
    else
      url "https://github.com/HuakunShen/space-lens/releases/download/cli-v#{version}/spacelens-x86_64-apple-darwin-v#{version}.tar.gz"
      sha256 "d49faf195bbbfb9c5ed108ceebcf3182919f63b83be9391199cddd9d8f81fab2"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/HuakunShen/space-lens/releases/download/cli-v#{version}/spacelens-aarch64-unknown-linux-musl-v#{version}.tar.gz"
      sha256 "77cbb0562d18ac908be802ee2d65c01588a7e30aae772d20919e2adf314aec9e"
    else
      url "https://github.com/HuakunShen/space-lens/releases/download/cli-v#{version}/spacelens-x86_64-unknown-linux-musl-v#{version}.tar.gz"
      sha256 "31cccd2ef5a2ede51bb0340671627098e41cdb8472f6184533ff1580ef0edf57"
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
