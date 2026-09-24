# The Homebrew cask for the Space Lens desktop app.
#
# This file lives in the Space Lens repository as the source of truth; each
# release, fill `version` and the two sha256 values from the real release
# artifacts (the `app-v*` tag job in workbench.yml publishes the dmg), then push
# this file to `HuakunShen/homebrew-tap` as `Casks/space-lens.rb` (the push is
# the owner's action — this repository never pushes).
#
# After a release exists, fill the sha256 values with:
#   shasum -a 256 <(curl -sL <dmg url>)   # or: brew fetch --cask after tapping
# then verify locally with:
#   brew audit --cask space-lens
#   brew install --cask HuakunShen/homebrew-tap/space-lens
#
# `auto_updates true` is load-bearing: the app updates itself from the release
# feed (tauri updater, minisign-verified), so `brew upgrade` must not fight it —
# Homebrew will recognise the newer installed version instead of reinstalling.

cask "space-lens" do
  arch arm: "aarch64", intel: "x64"

  version "0.1.0"
  sha256 arm:   "REPLACE_WITH_DMG_SHA256",
         intel: "REPLACE_WITH_DMG_SHA256"

  url "https://github.com/HuakunShen/space-lens/releases/download/app-v#{version}/Space.Lens_#{version}_#{arch}.dmg"
  name "Space Lens"
  desc "Local-first disk map: scan any folder and stage cleanup candidates"
  homepage "https://github.com/HuakunShen/space-lens"

  livecheck do
    url "https://github.com/HuakunShen/space-lens/releases/latest"
    strategy :github_latest do |json|
      json["tag_name"]&.sub(/^app-v/, "")
    end
  end

  auto_updates true

  app "Space Lens.app"

  zap trash: [
    "~/Library/Application Support/sh.huakun.spacelens.desktop",
    "~/Library/WebKit/sh.huakun.spacelens.desktop",
  ]
end
