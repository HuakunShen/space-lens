# The Homebrew formula for the Space Lens command-line interface (`spacelens`).
#
# This file lives in the Space Lens repository as the source of truth; after
# publishing `spacelens` to crates.io, fill `version` and `sha256`, then push
# this file to `HuakunShen/homebrew-tap` as `Formula/spacelens.rb` (the push is
# the owner's action — this repository never pushes).
#
# Fill the sha256 with:
#   shasum -a 256 <(curl -sL "https://crates.io/api/v1/crates/spacelens/<version>/download")
# then verify locally with:
#   brew audit HuakunShen/homebrew-tap/spacelens
#   brew install HuakunShen/homebrew-tap/spacelens
#
# The formula builds from the crates.io tarball, so it does not depend on the
# vendors/kuntu submodule; kuntu-scan is pulled from crates.io as declared in
# apps/cli/Cargo.toml.

class Spacelens < Formula
  desc "Scan disk usage and find cleanup candidates for development projects"
  homepage "https://github.com/HuakunShen/space-lens"
  url "https://crates.io/api/v1/crates/spacelens/0.2.8/download"
  sha256 "REPLACE_WITH_CRATE_TARBALL_SHA256"
  license "MIT"

  livecheck do
    url "https://crates.io/api/v1/crates/spacelens"
    strategy :crates_io
  end

  depends_on "rust" => :build
  # `space-lens dirty-git` shells out to git for `git status --porcelain`.
  uses_from_macos "git"

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/spacelens --version")
    assert_match "dry run", shell_output("#{bin}/spacelens clean #{testpath}")
  end
end
