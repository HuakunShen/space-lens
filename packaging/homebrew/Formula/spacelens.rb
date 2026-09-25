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
  version "0.2.9"
  license "MIT"

  livecheck do
    url "https://crates.io/api/v1/crates/spacelens"
    strategy :crates_io
  end

  # `spacelens dirty-git` shells out to git for `git status --porcelain`.
  uses_from_macos "git"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/HuakunShen/space-lens/releases/download/cli-v#{version}/spacelens-aarch64-apple-darwin-v#{version}.tar.gz"
      sha256 "9b35a4c2a5f4208acb492bdaaa3555069f13ccdfe68403a46b097a23ed9390c1"
    else
      url "https://github.com/HuakunShen/space-lens/releases/download/cli-v#{version}/spacelens-x86_64-apple-darwin-v#{version}.tar.gz"
      sha256 "f049234aa7ae48be0f998157cb4141d50dc1fb32d6c108567ffcf3b16a1a2eb9"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/HuakunShen/space-lens/releases/download/cli-v#{version}/spacelens-aarch64-unknown-linux-musl-v#{version}.tar.gz"
      sha256 "434ad7721a68a1c9af2d871f517867094adb835a4f8abc95c989928d61e58e39"
    else
      url "https://github.com/HuakunShen/space-lens/releases/download/cli-v#{version}/spacelens-x86_64-unknown-linux-musl-v#{version}.tar.gz"
      sha256 "79bc1c400a01572e4f04d8a58b1ca53252fc447183f7de5e05cf48eb44042cfb"
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
