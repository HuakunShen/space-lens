//! Live tests that sample the real operating system (§9 of
//! docs/memory-cleaner-plan.md). Nothing here is a fake: these are the checks
//! that prove the platform layer reads the machine it runs on, so they are
//! limited to the platforms the crate claims support for and are cheap enough
//! to keep in the default `cargo test` run.

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod portable {
  use spacelens_memory::platform::PortableSampler;
  use spacelens_memory::sampler::ProcessSampler;
  use spacelens_memory::{Platform, RawProcess, RawSample};
  use std::time::Duration;

  /// The one process every sample must contain.
  fn own_process(sample: &RawSample, pid: u32) -> &RawProcess {
    sample
      .processes
      .iter()
      .find(|process| process.pid == pid)
      .unwrap_or_else(|| panic!("sample does not contain the current process {pid}"))
  }

  #[test]
  fn two_samples_contain_the_current_process() {
    let own_pid = std::process::id();
    let mut sampler = PortableSampler::new();
    assert_ne!(
      sampler.platform(),
      Platform::Unsupported,
      "the portable sampler must name the platform it runs on"
    );

    let first = sampler.sample().expect("first sample");
    std::thread::sleep(Duration::from_millis(300));
    let second = sampler.sample().expect("second sample");

    assert!(
      first.processes.iter().all(|p| p.cpu_percent.is_none()),
      "the first sample has no previous tick to diff CPU usage against"
    );

    let first_own = own_process(&first, own_pid);
    let second_own = own_process(&second, own_pid);
    assert_eq!(
      first_own.start_time_us, second_own.start_time_us,
      "the identity of a live process must not change between samples"
    );
    assert!(
      second_own.start_time_us > 0,
      "a start time read from the operating system is never zero"
    );
    assert!(
      second_own.resident_bytes.is_some_and(|bytes| bytes > 0),
      "a running process has a resident set"
    );
    assert!(
      second_own.cpu_percent.is_some(),
      "the second sample has a previous tick to diff CPU usage against"
    );
    assert!(
      second_own
        .command
        .as_deref()
        .is_some_and(|command| !command.trim().is_empty()),
      "the sampler reads its own command line"
    );
    assert_ne!(
      second_own.state,
      spacelens_memory::ProcessState::Unknown,
      "a live process has a known scheduler state"
    );
    if cfg!(target_os = "linux") {
      assert!(
        second_own.threads.is_some_and(|threads| threads > 0),
        "linux reports the tasks of a live process"
      );
    }

    assert!(second.system.total_bytes > 0, "total memory is reported");
    assert!(
      second.system.available_bytes.is_some(),
      "the portable sampler reports available memory"
    );
    assert!(
      second.system.vm.is_none(),
      "page counters are a macOS-only enrichment"
    );
    assert!(
      second.system.pressure.available_percent.is_some(),
      "derived pressure carries the share it was derived from"
    );
  }
}

#[cfg(target_os = "macos")]
mod macos {
  use spacelens_memory::platform::{default_sampler, macos::MacSampler, PortableSampler};
  use spacelens_memory::sampler::ProcessSampler;
  use spacelens_memory::{Platform, PressureLevel, PressureSource, RawProcess, RawSample};

  /// The one process every sample must contain.
  fn own_process(sample: &RawSample, pid: u32) -> &RawProcess {
    sample
      .processes
      .iter()
      .find(|process| process.pid == pid)
      .unwrap_or_else(|| panic!("sample does not contain the current process {pid}"))
  }

  #[test]
  fn own_process_has_a_footprint_exact_start_and_kernel_pressure() {
    let own_pid = std::process::id();

    let mut portable = PortableSampler::new();
    let sysinfo_sample = portable.sample().expect("portable sample");
    let mut macos = MacSampler::new();
    let macos_sample = macos.sample().expect("macOS sample");

    let sysinfo_own = own_process(&sysinfo_sample, own_pid);
    let macos_own = own_process(&macos_sample, own_pid);

    let footprint = macos_own
      .footprint_bytes
      .expect("the current process always has a readable footprint");
    assert!(
      footprint > 0,
      "the footprint of a live process is never zero"
    );

    assert!(
      macos_own.threads.is_some_and(|threads| threads > 0),
      "the macOS sampler reads a live thread count"
    );

    let drift = macos_own.start_time_us.abs_diff(sysinfo_own.start_time_us);
    assert!(
      drift <= 1_000_000,
      "the exact start time ({}) must be within 1 s of sysinfo's ({})",
      macos_own.start_time_us,
      sysinfo_own.start_time_us
    );

    let pressure = &macos_sample.system.pressure;
    assert_ne!(
      pressure.level,
      PressureLevel::Unknown,
      "macOS always reports a kernel memory-pressure level"
    );
    assert_eq!(
      pressure.source,
      PressureSource::Kernel,
      "the macOS level comes from the kernel, not a derivation"
    );
    assert!(
      macos_sample.system.vm.is_some(),
      "the macOS sampler reads the VM page counters"
    );
  }

  #[test]
  fn default_sampler_is_the_macos_enrichment() {
    let own_pid = std::process::id();
    let mut sampler = default_sampler();
    assert_eq!(sampler.platform(), Platform::Macos);

    let sample = sampler.sample().expect("default sample");
    let own = own_process(&sample, own_pid);
    assert!(
      own.footprint_bytes.is_some_and(|bytes| bytes > 0),
      "the default macOS sampler reads per-process footprints"
    );
  }
}

/// The unix signaller against a real child process (§4.2 and T9 of
/// docs/memory-cleaner-plan.md): [`UnixSignaller`] reads a live process's exact
/// identity and delivers a real signal, and a one-process plan built through
/// [`PlanStore`] runs to `Exited` or, when the planned start time no longer
/// matches, to `IdentityChanged` without signalling.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod unix_signal {
  use spacelens_memory::fake::FakeClock;
  use spacelens_memory::group::build_groups;
  use spacelens_memory::plan::{AuditRing, PlanStore, SnapshotIndex};
  use spacelens_memory::platform::unix_signal::UnixSignaller;
  use spacelens_memory::platform::NullApps;
  use spacelens_memory::safety::SafetyContext;
  use spacelens_memory::sampler::{Signaller, Sleeper};
  use spacelens_memory::{
    MemoryBreakdown, MemoryPressure, MemorySnapshot, Platform, PressureLevel, PressureSource,
    RawProcess, TerminateExecuteRequest, TerminateMethod, TerminateMode, TerminateOrigin,
    TerminatePlanRequest, TerminateStatus, TerminateTarget,
  };
  use std::collections::{HashMap, HashSet};
  use std::process::{Child, Command};
  use std::time::Duration;

  /// The instant the plan and its execution are both dated at, so the plan has
  /// not expired by the time it runs.
  const NOW: i64 = 1_700_000_000_000;

  /// The session the plans are built in.
  const SESSION: &str = "sess_unix_signal_live";

  /// A `/bin/sleep 30` child that is always killed and reaped when it drops, so
  /// a failed assertion cannot leave a stray process behind.
  struct SleepChild {
    child: Option<Child>,
  }

  impl SleepChild {
    /// Spawn `/bin/sleep 30`, returning the guard and the child pid.
    fn spawn() -> (Self, u32) {
      let child = Command::new("/bin/sleep")
        .arg("30")
        .spawn()
        .expect("spawning /bin/sleep 30 must succeed");
      let pid = child.id();
      (SleepChild { child: Some(child) }, pid)
    }

    /// Whether the child is still running (not yet exited).
    fn is_alive(&mut self) -> bool {
      matches!(self.child.as_mut().map(Child::try_wait), Some(Ok(None)))
    }
  }

  impl Drop for SleepChild {
    fn drop(&mut self) {
      if let Some(mut child) = self.child.take() {
        let _ = child.kill();
        let _ = child.wait();
      }
    }
  }

  /// A [`Sleeper`] that really waits, so the execution poll loop gives the
  /// kernel time to deliver `SIGTERM` to the child.
  struct RealSleeper;

  impl Sleeper for RealSleeper {
    fn sleep_ms(&self, ms: u64) {
      std::thread::sleep(Duration::from_millis(ms));
    }
  }

  /// The raw process for a live child, read through the real signaller.
  fn raw_process(signaller: &UnixSignaller, pid: u32) -> RawProcess {
    let (start_time_us, uid, ppid, zombie) = signaller
      .identity(pid)
      .expect("a freshly spawned child has a readable identity");
    RawProcess {
      pid,
      ppid,
      start_time_us,
      uid,
      name: "sleep".to_string(),
      exe: Some("/bin/sleep".into()),
      command: Some("worker --job 42".to_string()),
      user: Some("hk".to_string()),
      state: spacelens_memory::ProcessState::Sleeping,
      threads: Some(2),
      listening_ports: vec![],
      cpu_percent: None,
      footprint_bytes: None,
      resident_bytes: None,
      virtual_bytes: None,
      zombie,
    }
  }

  /// The safety context to plan the target under.
  ///
  /// The service is this test process, but the target is not one of its
  /// ancestors, descendants or its group: a real service never plans a process
  /// it spawned. The self-child relation a directly spawned `/bin/sleep` would
  /// otherwise have (S6, covered by `tests/safety.rs`) is deliberately not
  /// modelled here, because this test is about the signaller's identity read
  /// and signal delivery, not the ancestry rules.
  fn planning_context(own_pid: u32, current_uid: Option<u32>) -> SafetyContext {
    SafetyContext {
      own_pid,
      current_uid,
      ancestors: HashSet::new(),
      descendants: HashSet::new(),
      own_group: None,
    }
  }

  /// The snapshot index and context for a single-process sample.
  fn index_for(
    raw: &RawProcess,
    own_pid: u32,
    current_uid: Option<u32>,
  ) -> (SnapshotIndex, SafetyContext) {
    let processes = [raw.clone()];
    let (groups, rows) = build_groups(&processes, &[], None, current_uid);
    let snapshot = MemorySnapshot {
      sequence: 1,
      sampled_at: "2026-10-18T00:00:00.000Z".to_string(),
      interval_ms: 1_000,
      platform: if cfg!(target_os = "macos") {
        Platform::Macos
      } else {
        Platform::Linux
      },
      breakdown: MemoryBreakdown {
        total_bytes: 16_000_000_000,
        used_bytes: 8_000_000_000,
        free_bytes: 4_000_000_000,
        available_bytes: Some(6_000_000_000),
        app_bytes: None,
        wired_bytes: None,
        compressed_bytes: None,
        cached_files_bytes: None,
        swap_total_bytes: 0,
        swap_used_bytes: 0,
      },
      pressure: MemoryPressure {
        level: PressureLevel::Normal,
        source: PressureSource::Kernel,
        available_percent: Some(70.0),
      },
      groups,
      processes: rows,
      notes: Vec::new(),
    };
    let index = SnapshotIndex::new(&snapshot, &processes, &HashMap::new(), NOW);
    let context = planning_context(own_pid, current_uid);
    (index, context)
  }

  /// A generator of 16-character `[0-9a-zA-Z]` bodies under `prefix`, matching
  /// the `tplan_`/`aud_` identifier patterns.
  fn counter(prefix: &'static str) -> impl FnMut() -> String {
    let mut n = 0u64;
    move || {
      n += 1;
      format!("{prefix}{n:016x}")
    }
  }

  #[test]
  fn graceful_plan_sigterms_a_live_child_to_exit() {
    let signaller = UnixSignaller::new();
    let (mut child, pid) = SleepChild::spawn();
    let raw = raw_process(&signaller, pid);
    let (index, context) = index_for(&raw, signaller.own_pid(), signaller.current_uid());

    let mut store = PlanStore::new();
    let mut plan_ids = counter("tplan_");
    let request = TerminatePlanRequest {
      targets: vec![TerminateTarget::Process {
        pid,
        start_time_us: raw.start_time_us,
      }],
      mode: TerminateMode::Graceful,
      origin: TerminateOrigin::Manual,
    };
    let plan = store
      .build(SESSION, &request, &index, &context, NOW, &mut plan_ids)
      .expect("building a one-process plan must succeed");
    assert_eq!(plan.entries.len(), 1, "the sleep child must be planned");
    assert!(
      plan.refused.is_empty(),
      "a plain sleep child is not refused"
    );

    let clock = FakeClock::new(NOW);
    let mut apps = NullApps;
    let mut audit = AuditRing::new();
    let mut audit_ids = counter("aud_");
    let outcome = store
      .execute(
        SESSION,
        &TerminateExecuteRequest {
          plan_id: plan.plan_id.clone(),
          confirm: true,
        },
        &context,
        &signaller,
        &mut apps,
        &clock,
        &RealSleeper,
        &mut audit,
        &mut audit_ids,
      )
      .expect("executing the plan must succeed");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
      outcome.results[0].status,
      TerminateStatus::Exited,
      "the child must have exited after the graceful signal"
    );
    assert_eq!(outcome.results[0].method, TerminateMethod::Sigterm);
    assert_eq!(audit.len(), 1, "one executed entry is audited");
    assert!(
      !child.is_alive(),
      "a reaped child is no longer running when the guard drops"
    );
  }

  #[test]
  fn a_tampered_start_time_is_identity_changed_and_nothing_is_signalled() {
    let signaller = UnixSignaller::new();
    let (mut child, pid) = SleepChild::spawn();
    let live = raw_process(&signaller, pid);

    // Tamper the plan index: pretend the process started one microsecond later
    // than it did, so the execution-time identity read cannot match (S15).
    let mut tampered = live.clone();
    tampered.start_time_us += 1;

    let (index, context) = index_for(&tampered, signaller.own_pid(), signaller.current_uid());

    let mut store = PlanStore::new();
    let mut plan_ids = counter("tplan_");
    let request = TerminatePlanRequest {
      targets: vec![TerminateTarget::Process {
        pid,
        start_time_us: tampered.start_time_us,
      }],
      mode: TerminateMode::Graceful,
      origin: TerminateOrigin::Manual,
    };
    let plan = store
      .build(SESSION, &request, &index, &context, NOW, &mut plan_ids)
      .expect("building a one-process plan must succeed");
    assert_eq!(plan.entries.len(), 1);

    let clock = FakeClock::new(NOW);
    let mut apps = NullApps;
    let mut audit = AuditRing::new();
    let mut audit_ids = counter("aud_");
    let outcome = store
      .execute(
        SESSION,
        &TerminateExecuteRequest {
          plan_id: plan.plan_id.clone(),
          confirm: true,
        },
        &context,
        &signaller,
        &mut apps,
        &clock,
        &RealSleeper,
        &mut audit,
        &mut audit_ids,
      )
      .expect("executing the plan must succeed");

    assert_eq!(outcome.results.len(), 1);
    assert_eq!(
      outcome.results[0].status,
      TerminateStatus::IdentityChanged,
      "a start time that no longer matches must not be signalled (S15)"
    );
    assert!(
      child.is_alive(),
      "an identity-changed entry must not be signalled, so the child is still alive"
    );
  }

  #[test]
  fn default_signaller_reads_the_current_user() {
    let signaller = spacelens_memory::platform::default_signaller();
    assert!(
      signaller.current_uid().is_some(),
      "a unix build can always read the current uid"
    );
    let own = signaller
      .identity(signaller.own_pid())
      .expect("the current process has an identity");
    assert!(own.0 > 0, "the current process started after the epoch");
  }
}

/// The AppKit application directory against the live desktop (T11 of
/// docs/memory-cleaner-plan.md). These read the real `NSWorkspace` application
/// list, so they need a logged-in graphical session and are `#[ignore]`d out of
/// the default run; run them manually with
/// `cargo test -p spacelens-memory --features appkit -- --ignored appkit`.
#[cfg(all(target_os = "macos", feature = "appkit"))]
mod appkit {
  use objc2_app_kit::NSRunningApplication;
  use objc2_foundation::NSString;
  use spacelens_memory::platform::appkit::AppKitApps;
  use spacelens_memory::platform::default_sampler;
  use spacelens_memory::raw::QuitAck;
  use spacelens_memory::sampler::AppDirectory;
  use std::process::Command;
  use std::thread::sleep;
  use std::time::Duration;

  /// Finder runs in every logged-in graphical session.
  const FINDER_BUNDLE_ID: &str = "com.apple.finder";
  /// The manual quit check targets an ordinary, save-aware AppKit application.
  const TEXT_EDIT_BUNDLE_ID: &str = "com.apple.TextEdit";

  #[test]
  #[ignore = "live: reads the AppKit running-application list; run manually in a GUI session"]
  fn appkit_apps_are_non_empty_and_contain_finder() {
    let mut apps = AppKitApps::new();
    let running = apps.apps();

    assert!(
      !running.is_empty(),
      "a logged-in session always has running applications"
    );
    assert!(
      running
        .iter()
        .any(|app| app.bundle_id.as_deref() == Some(FINDER_BUNDLE_ID)),
      "Finder is running in every logged-in session"
    );
    assert!(
      running.iter().all(|app| app.pid > 0),
      "AppKit never reports a non-positive pid for a running application"
    );
  }

  #[test]
  #[ignore = "live: needs the real process list; run manually in a GUI session"]
  fn appkit_frontmost_pid_is_in_the_process_list() {
    let mut apps = AppKitApps::new();
    let frontmost = apps
      .frontmost_pid()
      .expect("a logged-in graphical session has a frontmost application");

    let mut sampler = default_sampler();
    let sample = sampler.sample().expect("a live process sample");
    assert!(
      sample
        .processes
        .iter()
        .any(|process| process.pid == frontmost),
      "the frontmost application ({frontmost}) must be one of the sampled processes"
    );
  }

  #[test]
  #[ignore = "live: asks a real application to quit; run manually, watch for a TCC prompt"]
  fn appkit_request_quit_targets_text_edit() {
    let mut apps = AppKitApps::new();

    // The test runner is not an application, so AppKit knows no such app.
    assert_eq!(
      apps
        .request_quit(std::process::id())
        .expect("a quit request never fails"),
      QuitAck::NotAnApp,
      "a plain process is not an application"
    );

    ensure_text_edit_running();
    let pid = text_edit_pid().expect("TextEdit is running after it was launched");
    let ack = apps.request_quit(pid).expect("a quit request never fails");
    eprintln!("request_quit(TextEdit {pid}) -> {ack:?}");
    assert_eq!(
      ack,
      QuitAck::Sent,
      "AppKit accepted the quit request for TextEdit"
    );
  }

  /// Launch TextEdit through Launch Services if it is not already running.
  fn ensure_text_edit_running() {
    if text_edit_pid().is_some() {
      return;
    }
    Command::new("/usr/bin/open")
      .args(["-a", "TextEdit"])
      .status()
      .expect("opening TextEdit must succeed");
    for _ in 0..50 {
      if text_edit_pid().is_some() {
        return;
      }
      sleep(Duration::from_millis(100));
    }
  }

  /// The pid of a running TextEdit, read straight from Launch Services rather
  /// than the possibly stale `NSWorkspace` cache.
  ///
  /// `-[NSRunningApplication processIdentifier]` sits behind the `libc` feature
  /// of `objc2-app-kit`, which the crate does not enable, so the same message is
  /// sent directly here.
  fn text_edit_pid() -> Option<u32> {
    let bundle_id = NSString::from_str(TEXT_EDIT_BUNDLE_ID);
    let running = NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id);
    let app = running.to_vec().into_iter().next()?;
    let pid: i32 = unsafe { objc2::msg_send![&app, processIdentifier] };
    u32::try_from(pid).ok()
  }
}
