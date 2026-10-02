//! macOS surfaces and compact controls shared by the native shell.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{h_flex, ActiveTheme as _, Icon, Sizable as _, Theme, ThemeMode};
use gpui_kit::*;

pub const SIDEBAR_WIDTH: f32 = 184.;
pub const TOOLBAR_HEIGHT: f32 = 56.;

pub fn surface(cx: &App, light: u32, dark: u32) -> Hsla {
  rgb(if cx.theme().mode.is_dark() {
    dark
  } else {
    light
  })
  .into()
}

pub fn toolbar(cx: &App) -> Hsla {
  surface(cx, 0xf5f5f5, 0x2b2b2b)
}
pub fn sidebar(cx: &App) -> Hsla {
  surface(cx, 0xe9e9ed, 0x29292d)
}
pub fn inspector(cx: &App) -> Hsla {
  surface(cx, 0xfafafa, 0x252525)
}
pub fn stripe(cx: &App) -> Hsla {
  surface(cx, 0xf4f4f5, 0x2b2b2d)
}
pub fn selected(cx: &App) -> Hsla {
  surface(cx, 0xd8d8df, 0x4c4c51)
}
pub fn segment_track(cx: &App) -> Hsla {
  surface(cx, 0xdedee0, 0x3d3d40)
}
pub fn segment_selected(cx: &App) -> Hsla {
  surface(cx, 0xffffff, 0x68686d)
}

pub fn sidebar_button(
  id: impl Into<ElementId>,
  label: impl Into<SharedString>,
  icon: IconName,
  cx: &App,
) -> Button {
  let label = label.into();
  Button::new(id)
    .ghost()
    .small()
    .w_full()
    .h(px(30.))
    .accessibility_label(label.clone())
    .child(
      h_flex()
        .w_full()
        .min_w_0()
        .gap(px(8.))
        .child(Icon::new(icon).small().text_color(cx.theme().primary))
        .child(
          div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(13.))
            .child(label),
        ),
    )
}

pub fn apply_macos_theme(mode: ThemeMode, cx: &mut App) {
  Theme::change(mode, None, cx);
  Theme::update(cx, |theme| {
    let dark = mode.is_dark();
    let color = |light, dark_color| Hsla::from(rgb(if dark { dark_color } else { light }));
    theme.font_family = ".SystemUIFont".into();
    theme.font_size = px(13.);
    theme.radius = px(5.);
    theme.radius_lg = px(9.);
    theme.colors.background = color(0xffffff, 0x202020);
    theme.colors.foreground = color(0x202024, 0xeeeeef);
    theme.colors.muted_foreground = color(0x727278, 0xaaaab0);
    theme.colors.border = color(0xdcdcdf, 0x414144);
    // `input` is the control border, also used by unchecked checkboxes.
    theme.colors.input = color(0xc6c6cb, 0x535357);
    theme.colors.primary = color(0x007aff, 0x0a84ff);
    theme.colors.primary_foreground = rgb(0xffffff).into();
    theme.colors.button_primary = theme.colors.primary;
    theme.colors.button_primary_foreground = rgb(0xffffff).into();
    theme.colors.button_primary_hover = color(0x1687ff, 0x2693ff);
    theme.colors.button_primary_active = color(0x006de5, 0x0075ee);
    theme.colors.button = color(0xffffff, 0x454548);
    theme.colors.button_foreground = theme.colors.foreground;
    theme.colors.button_hover = color(0xf0f0f3, 0x515156);
    theme.colors.list_even = color(0xf4f4f5, 0x2b2b2d);
    theme.colors.list_hover = color(0xe8eef7, 0x343c48);
    theme.colors.list_active = color(0xd8d8df, 0x4c4c51);
    theme.colors.sidebar = color(0xe9e9ed, 0x29292d);
    theme.colors.sidebar_foreground = theme.colors.foreground;
    theme.colors.ring = theme.colors.primary.opacity(0.5);
  });
}

pub fn theme_toggle_button(cx: &App) -> Button {
  let dark = cx.theme().mode.is_dark();
  Button::new("theme-toggle")
    .ghost()
    .small()
    .accessibility_label("Switch appearance")
    .tooltip(if dark {
      "Switch to light appearance"
    } else {
      "Switch to dark appearance"
    })
    .icon(Icon::new(if dark { IconName::Sun } else { IconName::Moon }).small())
    .on_click(|_, _, cx| {
      let mode = if cx.theme().mode.is_dark() {
        ThemeMode::Light
      } else {
        ThemeMode::Dark
      };
      apply_macos_theme(mode, cx);
    })
}
