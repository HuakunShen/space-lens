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

  version "0.8.1"
  sha256 arm:   "e5fa1c2f796193823aac9e0caf4bd7338278339054f84fa059e8d3aeb8926eb4",
         intel: "a036dfc5ea3fe33bc53d4a0b2e4c73a372948b7f8e0235143eb73631bdb8e932"

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
