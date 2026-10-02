//! Minimal gpui-kit window bootstrap — the app shell milestone.
//!
//! `open_window` wraps the content view in the kit's `Root` overlay layer, so
//! the build closure must return the content view itself, not another `Root`.
//! `init` must run before any window or component is constructed: it registers
//! the component theme and default assets the widgets below depend on.

mod app_state;
mod session;
mod store;
mod views;

use app_state::AppState;
use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::*;
use views::theme::apply_macos_theme;

actions!(spacelens, [Quit]);

fn main() {
  application().with_assets(app_assets()).run(|cx| {
    init(cx);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
    cx.set_menus([Menu::new("Space Lens").items([MenuItem::action("Quit Space Lens", Quit)])]);
    let options = WindowOptions {
      window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
        None,
        size(px(1180.), px(760.)),
        cx,
      ))),
      window_min_size: Some(size(px(820.), px(580.))),
      titlebar: Some(TitlebarOptions {
        title: Some("Space Lens".into()),
        appears_transparent: true,
        traffic_light_position: Some(point(px(18.), px(21.))),
      }),
      app_owns_titlebar_drag: true,
      window_background: WindowBackgroundAppearance::Blurred,
      ..Default::default()
    };
    open_window(options, cx, |window, cx| {
      window.set_rem_size(px(14.));
      Theme::sync_system_appearance(Some(window), cx);
      let mode = match std::env::var("SPACLENS_GPUI_THEME").as_deref() {
        Ok("dark") => ThemeMode::Dark,
        Ok("light") => ThemeMode::Light,
        _ => cx.theme().mode,
      };
      apply_macos_theme(mode, cx);
      window
        .observe_window_appearance(|window, cx| {
          Theme::sync_system_appearance(Some(window), cx);
          apply_macos_theme(cx.theme().mode, cx);
        })
        .detach();
      cx.new(|cx| AppState::new(window, cx))
    })
    .expect("Failed to open window");
  });
}

fn app_assets() -> assets::AllAssets {
  assets::AllAssets
}

#[cfg(test)]
mod tests {
  use gpui_kit::assets::IconName;
  use gpui_kit::AssetSource;

  #[test]
  fn native_navigation_icons_are_embedded_in_the_application() {
    let source = super::app_assets();
    for icon in [
      IconName::FolderSearch,
      IconName::FolderPlus,
      IconName::House,
      IconName::BrushCleaning,
      IconName::GitBranch,
    ] {
      let path = icon.path();
      assert!(
        source.load(&path).unwrap().is_some(),
        "missing bundled icon: {path}"
      );
    }
  }
}
