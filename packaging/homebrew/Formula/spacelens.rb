# The Homebrew formula for the Space Lens command-line interface (`spacelens`).
#
# This file lives in the Space Lens repository as the source of truth; each
# release, fill `version` and the sha256 values from the real release
# artifacts (the `v*` tag job in cli-release.yml publishes the tarballs), then
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
  version "0.2.8"
  license "MIT"

  livecheck do
    url "https://crates.io/api/v1/crates/spacelens"
    strategy :crates_io
  end

  # `spacelens dirty-git` shells out to git for `git status --porcelain`.
  uses_from_macos "git"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/HuakunShen/space-lens/releases/download/v#{version}/spacelens-aarch64-apple-darwin-v#{version}.tar.gz"
      sha256 "REPLACE_WITH_ARM64_MACOS_SHA256"
    else
      url "https://github.com/HuakunShen/space-lens/releases/download/v#{version}/spacelens-x86_64-apple-darwin-v#{version}.tar.gz"
      sha256 "REPLACE_WITH_X64_MACOS_SHA256"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/HuakunShen/space-lens/releases/download/v#{version}/spacelens-aarch64-unknown-linux-musl-v#{version}.tar.gz"
      sha256 "REPLACE_WITH_ARM64_LINUX_SHA256"
    else
      url "https://github.com/HuakunShen/space-lens/releases/download/v#{version}/spacelens-x86_64-unknown-linux-musl-v#{version}.tar.gz"
      sha256 "REPLACE_WITH_X64_LINUX_SHA256"
    end
  end

  def install
    arch = Hardware::CPU.intel? ? "x86_64" : "aarch64"
    os = OS.mac? ? "apple-darwin" : "unknown-linux-musl"
    bin.install "spacelens-#{arch}-#{os}-v#{version}/spacelens"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/spacelens --version")
    assert_match "dry run", shell_output("#{bin}/spacelens clean #{testpath}")
  end
end
