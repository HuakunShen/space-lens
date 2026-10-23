//! The AppKit application directory (T11 of docs/memory-cleaner-plan.md): the
//! macOS [`AppDirectory`] that turns the running-application list into the
//! bundle identity the grouping step needs, tells the sampler which application
//! is frontmost, and asks an application to quit through `NSRunningApplication`
//! so a real app can prompt its user to save before it exits (§4.2, S10).
//!
//! AppKit only applies launch, quit and activation notifications to
//! `NSWorkspace.runningApplications` while a run loop is running, so
//! [`pump_main_run_loop`] runs the current thread's run loop for an instant and
//! is called at the start of every read. It is a no-op off the main thread: the
//! desktop host already runs its own main run loop, and a background sample must
//! not drive another thread's loop. A read that must never be stale goes to
//! `+runningApplicationsWithBundleIdentifier:` instead, which queries Launch
//! Services directly.
//!
//! # Safety
//!
//! This is one of the two modules allowed to use `unsafe` (`lib.rs` denies it
//! crate-wide and this `allow` narrows the exception to this file). Every call
//! below is an Objective-C message to a class in AppKit or Foundation, both
//! linked for a macOS build with the `appkit` feature.

#![allow(unsafe_code)]

use crate::error::MemoryError;
use crate::raw::{AppInfo, QuitAck};
use crate::sampler::AppDirectory;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};
use objc2_foundation::NSDate;

/// The AppKit-backed [`AppDirectory`] (§4.2).
#[derive(Clone, Copy, Debug, Default)]
pub struct AppKitApps;

impl AppKitApps {
  /// An application directory backed by the shared `NSWorkspace`.
  pub fn new() -> Self {
    AppKitApps
  }
}

impl AppDirectory for AppKitApps {
  fn apps(&mut self) -> Vec<AppInfo> {
    pump_main_run_loop();
    NSWorkspace::sharedWorkspace()
      .runningApplications()
      .to_vec()
      .into_iter()
      .filter_map(|app| app_info(&app))
      .collect()
  }

  fn frontmost_pid(&mut self) -> Option<u32> {
    pump_main_run_loop();
    NSWorkspace::sharedWorkspace()
      .frontmostApplication()
      .as_deref()
      .and_then(pid_of)
  }

  fn request_quit(&mut self, pid: u32) -> Result<QuitAck, MemoryError> {
    let Ok(pid) = i32::try_from(pid) else {
      return Ok(QuitAck::NotAnApp);
    };
    let app: Option<Retained<NSRunningApplication>> = unsafe {
      msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid]
    };
    match app {
      Some(app) if app.terminate() => Ok(QuitAck::Sent),
      // No such application, or AppKit could not hand the quit to one: either
      // way the caller falls back to `SIGTERM` (S10).
      _ => Ok(QuitAck::NotAnApp),
    }
  }
}

/// The [`AppInfo`] for one running application, or `None` for an application
/// without a pid (AppKit reports `-1` for those, and §4.2 has no field for
/// them).
fn app_info(app: &NSRunningApplication) -> Option<AppInfo> {
  Some(AppInfo {
    pid: pid_of(app)?,
    bundle_id: app.bundleIdentifier().map(|id| id.to_string()),
    name: app.localizedName().map(|name| name.to_string()),
    regular: app.activationPolicy() == NSApplicationActivationPolicy::Regular,
  })
}

/// The pid of a running application.
///
/// `-[NSRunningApplication processIdentifier]` sits behind the `libc` feature of
/// `objc2-app-kit`, which this crate does not enable; the message returns the
/// same `pid_t` the typed method would, and `-1` (an application without a pid)
/// becomes `None`.
fn pid_of(app: &NSRunningApplication) -> Option<u32> {
  let pid: i32 = unsafe { msg_send![app, processIdentifier] };
  u32::try_from(pid).ok()
}

/// Runs the current thread's run loop for an instant when the caller is on the
/// main thread (§4.2 and T11), so `NSWorkspace.runningApplications` reflects
/// launches and quits that happened since the last run-loop turn.
///
/// Off the main thread this does nothing: the desktop host runs its own main run
/// loop, and a background sample must not drive a run loop that AppKit was never
/// attached to.
pub fn pump_main_run_loop() {
  if !is_main_thread() {
    return;
  }
  let run_loop: Retained<AnyObject> = unsafe { msg_send![class!(NSRunLoop), currentRunLoop] };
  let now = NSDate::now();
  let _: () = unsafe { msg_send![&*run_loop, runUntilDate: &*now] };
}

/// Whether the calling thread is this process's main thread.
fn is_main_thread() -> bool {
  unsafe { msg_send![class!(NSThread), isMainThread] }
}
