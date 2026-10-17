//! Live tests that sample the real operating system (§9 of
//! docs/memory-cleaner-plan.md). Nothing here is a fake: these are the checks
//! that prove the platform layer reads the machine it runs on, so they are
//! limited to the platforms the crate claims support for and are cheap enough
//! to keep in the default `cargo test` run.

#[cfg(any(target_os = "macos", target_os = "linux"))]
mod portable {
  use spacelens_memory::platform::default_sampler;
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
    let mut sampler = default_sampler();
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
