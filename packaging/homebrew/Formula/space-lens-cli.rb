# The Homebrew formula for the Space Lens command-line interface.
#
# This file lives in the Space Lens repository as the source of truth; after
# publishing `space-lens-cli` to crates.io, fill `version` and `sha256`, then
# push this file to `HuakunShen/homebrew-tap` as `Formula/space-lens-cli.rb`
# (the push is the owner's action — this repository never pushes).
#
# Fill the sha256 with:
#   shasum -a 256 <(curl -sL "https://crates.io/api/v1/crates/space-lens-cli/<version>/download")
# then verify locally with:
#   brew audit HuakunShen/homebrew-tap/space-lens-cli
#   brew install HuakunShen/homebrew-tap/space-lens-cli
#
# The formula builds from the crates.io tarball, so it does not depend on the
# vendors/kuntu submodule; kuntu-scan is pulled from crates.io as declared in
# apps/cli/Cargo.toml.

class SpaceLensCli < Formula
  desc "Scan disk usage and find cleanup candidates for development projects"
  homepage "https://github.com/HuakunShen/space-lens"
  url "https://crates.io/api/v1/crates/space-lens-cli/0.2.8/download"
  sha256 "REPLACE_WITH_CRATE_TARBALL_SHA256"
  license "MIT"

  livecheck do
    url "https://crates.io/api/v1/crates/space-lens-cli"
    strategy :crates_io
  end

  depends_on "rust" => :build
  # `space-lens dirty-git` shells out to git for `git status --porcelain`.
  uses_from_macos "git"

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/space-lens --version")
    assert_match "dry run", shell_output("#{bin}/space-lens clean #{testpath}")
  end
end
