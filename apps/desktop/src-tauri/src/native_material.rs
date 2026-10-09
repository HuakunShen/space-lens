//! The native host owns material capability. A macOS styling preference cannot
//! turn CSS blur into Liquid Glass: only a successfully installed AppKit view
//! publishes `glass` or `vibrancy` to the document.

use tauri::{Manager, WebviewWindowBuilder};

const BOOTSTRAP: &str = include_str!("native_material.js");

pub fn create_main_window(app: &mut tauri::App) -> tauri::Result<()> {
  #[cfg(target_os = "macos")]
  let bootstrap = macos::initialization_script();
  #[cfg(not(target_os = "macos"))]
  let bootstrap = BOOTSTRAP.to_string();
  let config = app
    .config()
    .app
    .windows
    .iter()
    .find(|window| window.label == "main")
    .expect("main window configuration");
  let window = WebviewWindowBuilder::from_config(app, config)?
    .initialization_script(bootstrap)
    .on_page_load(|window, payload| {
      #[cfg(target_os = "macos")]
      if payload.event() == tauri::webview::PageLoadEvent::Finished {
        // Refresh appearance/accent after reload, including a first navigation
        // which started before the material's WKUserScript was installed.
        macos::refresh(window.app_handle());
      }
      #[cfg(not(target_os = "macos"))]
      let _ = (window, payload);
    })
    .build()?;
  #[cfg(target_os = "macos")]
  {
    let ready = window.clone();
    window.with_webview(move |platform| {
      // with_webview executes on the main thread and keeps Tauri's platform
      // handles alive for this callback. No raw AppKit pointers escape it.
      unsafe { macos::install(&platform) };
      macos::observe_preferences(ready.app_handle().clone());
      let _ = ready.show();
    })?;
  }
  #[cfg(not(target_os = "macos"))]
  window.show()?;
  Ok(())
}

pub fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
  #[cfg(target_os = "macos")]
  match event {
    tauri::WindowEvent::ThemeChanged(_) | tauri::WindowEvent::Focused(true) => {
      macos::refresh(window.app_handle());
    }
    tauri::WindowEvent::Destroyed => macos::remove_observers(),
    _ => {}
  }
  #[cfg(not(target_os = "macos"))]
  let _ = (window, event);
}

#[cfg(target_os = "macos")]
mod macos {
  use block2::RcBlock;
  use objc2::{
    msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject, ProtocolObject},
    sel, ClassType, MainThreadMarker, MainThreadOnly,
  };
  use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSAutoresizingMaskOptions,
    NSColor, NSColorSpace, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindow,
  };
  use objc2_foundation::{
    NSArray, NSDistributedNotificationCenter, NSNotification, NSNotificationCenter,
    NSObjectProtocol, NSOperationQueue, NSProcessInfo, NSString,
  };
  use std::{cell::RefCell, ptr::NonNull};
  use tauri::Manager;

  struct Observer {
    center: Retained<NSNotificationCenter>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
  }
  // AppKit objects stay on the main thread. The centers retain the callback;
  // these tokens are removed when the main window dies, breaking AppHandle
  // retention. Callbacks resolve current Tauri handles instead of saving raw
  // WKWebView pointers, so a delayed notification cannot use a dead window.
  thread_local! { static OBSERVERS: RefCell<Vec<Observer>> = const { RefCell::new(Vec::new()) }; }

  fn glass_class() -> Option<&'static AnyClass> {
    if NSProcessInfo::processInfo()
      .operatingSystemVersion()
      .majorVersion
      < 26
    {
      return None;
    }
    AnyClass::get(c"NSGlassEffectView")
  }

  pub fn initialization_script() -> String {
    // Setup runs on the AppKit thread. Publish OS/appearance/accent before the
    // first document, but keep capability at none until material installation.
    unsafe {
      format!(
        "{}\nwindow.__nativeHost.apply({});",
        super::BOOTSTRAP,
        state("none")
      )
    }
  }

  unsafe fn backdrop(window: &NSWindow) -> &'static str {
    let Some(content) = window.contentView() else {
      return "none";
    };
    if glass_class().is_some_and(|class| content.isKindOfClass(class)) {
      "glass"
    } else if content.isKindOfClass(NSVisualEffectView::class()) {
      "vibrancy"
    } else {
      "none"
    }
  }

  /// The native window owns the material; its content view owns the original
  /// Tauri content view. Retained locals bridge removal/reparenting so neither
  /// view can deallocate in between. Keeping Tauri's original container (and
  /// WKWebView inside it) preserves its resize, keyboard and drag machinery.
  pub unsafe fn install(platform: &tauri::webview::PlatformWebview) {
    let mtm = MainThreadMarker::new().expect("AppKit main thread");
    let webview = &*platform.inner().cast::<AnyObject>();
    let window: Option<Retained<NSWindow>> = msg_send![webview, window];
    let Some(window) = window else {
      return;
    };
    let transparent: bool = msg_send![webview, respondsToSelector: sel!(_setDrawsBackground:)];
    if !transparent {
      // This private WKWebView transparency setter is the only private hook.
      // Guard it and keep an opaque, readable window if WebKit removes it.
      push(webview, &window, false);
      return;
    }
    let Some(content) = window.contentView() else {
      return;
    };
    let bounds = content.bounds();
    let material: Retained<NSView> = if let Some(class) = glass_class() {
      let glass: Retained<NSView> = msg_send![class, new];
      let supports_content: bool = msg_send![&*glass, respondsToSelector: sel!(setContentView:)];
      if supports_content {
        glass.setFrame(bounds);
        let _: () = msg_send![&*glass, setContentView: &*content];
        glass
      } else {
        vibrancy(mtm, &content)
      }
    } else {
      vibrancy(mtm, &content)
    };
    material.setAutoresizingMask(
      NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    content.setAutoresizingMask(
      NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    window.setContentView(Some(&material));
    let _: () = msg_send![webview, _setDrawsBackground: false];
    let supports_under_page: bool =
      msg_send![webview, respondsToSelector: sel!(setUnderPageBackgroundColor:)];
    if supports_under_page {
      let _: () = msg_send![webview, setUnderPageBackgroundColor: &*NSColor::clearColor()];
    }
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setTitlebarAppearsTransparent(true);

    // Initial navigation can already be in flight when setup runs. Add the
    // audited state at document start for subsequent navigations, then apply
    // it to the initial document while the native window is still hidden.
    let source = NSString::from_str(&format!(
      "{}\nwindow.__nativeHost.apply({});",
      super::BOOTSTRAP,
      state(backdrop(&window))
    ));
    let script_class = AnyClass::get(c"WKUserScript").expect("WKUserScript class");
    let script_alloc: Allocated<AnyObject> = msg_send![script_class, alloc];
    let script: Retained<AnyObject> = msg_send![script_alloc, initWithSource: &*source, injectionTime: 0_isize, forMainFrameOnly: true];
    let controller = &*platform.controller().cast::<AnyObject>();
    let _: () = msg_send![controller, addUserScript: &*script];
    push(webview, &window, true);
    eprintln!(
      "native material: macOS {} / {}",
      NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion,
      backdrop(&window)
    );
  }

  unsafe fn vibrancy(mtm: MainThreadMarker, content: &NSView) -> Retained<NSView> {
    let effect =
      NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), content.bounds());
    effect.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::FollowsWindowActiveState);
    effect.addSubview(content);
    Retained::into_super(effect)
  }

  unsafe fn state(backdrop: &str) -> String {
    let mtm = MainThreadMarker::new().expect("AppKit main thread");
    let appearance = NSApplication::sharedApplication(mtm).effectiveAppearance();
    let names = NSArray::from_slice(&[NSAppearanceNameDarkAqua, NSAppearanceNameAqua]);
    let dark = appearance
      .bestMatchFromAppearancesWithNames(&names)
      .is_some_and(|name| &*name == NSAppearanceNameDarkAqua);
    let accent = NSColor::controlAccentColor()
      .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
      .unwrap_or_else(NSColor::systemBlueColor);
    let hex = format!(
      "#{:02x}{:02x}{:02x}",
      (accent.redComponent() * 255.0).round() as u8,
      (accent.greenComponent() * 255.0).round() as u8,
      (accent.blueComponent() * 255.0).round() as u8
    );
    serde_json::json!({"macos": NSProcessInfo::processInfo().operatingSystemVersion().majorVersion.to_string(), "dark": dark, "accent": hex, "backdrop": backdrop}).to_string()
  }

  unsafe fn push(webview: &AnyObject, window: &NSWindow, installed: bool) {
    let script = NSString::from_str(&format!(
      "{}\nwindow.__nativeHost.apply({});",
      super::BOOTSTRAP,
      state(if installed { backdrop(window) } else { "none" })
    ));
    let _: () = msg_send![webview, evaluateJavaScript: &*script, completionHandler: std::ptr::null::<AnyObject>()];
  }

  pub fn refresh(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
      let _ = window.with_webview(|platform| unsafe {
        let webview = &*platform.inner().cast::<AnyObject>();
        let window: Option<Retained<NSWindow>> = msg_send![webview, window];
        if let Some(window) = window {
          push(webview, &window, backdrop(&window) != "none");
        }
      });
    }
  }

  pub fn observe_preferences(app: tauri::AppHandle) {
    OBSERVERS.with(|observers| {
      let mut observers = observers.borrow_mut();
      if !observers.is_empty() {
        return;
      }
      let center: Retained<NSNotificationCenter> =
        Retained::into_super(NSDistributedNotificationCenter::defaultCenter());
      for name in [
        "AppleColorPreferencesChangedNotification",
        "AppleInterfaceThemeChangedNotification",
      ] {
        let app = app.clone();
        let callback = RcBlock::new(move |_: NonNull<NSNotification>| refresh(&app));
        let token = unsafe {
          center.addObserverForName_object_queue_usingBlock(
            Some(&NSString::from_str(name)),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &callback,
          )
        };
        observers.push(Observer {
          center: center.clone(),
          token,
        });
      }
    });
  }

  pub fn remove_observers() {
    OBSERVERS.with(|observers| {
      for observer in observers.borrow_mut().drain(..) {
        unsafe {
          observer.center.removeObserver(observer.token.as_ref());
        }
      }
    });
  }
}
