//! The `spacelens memory` subcommands (T12 of docs/memory-cleaner-plan.md).
//!
//! [`MemoryCommand::Snapshot`] prints one wire snapshot after two samples taken
//! 500 ms apart (the first only warms the CPU counters the sampler diffs
//! against); [`MemoryCommand::Stdio`] serves the line protocol the HTTP host
//! speaks (§4.6) through the real [`MemoryService`]; [`MemoryCommand::Bench`]
//! times the sampler tick and prints its percentiles as JSON.
//!
//! `snapshot` and `bench` assemble a snapshot from the same public building
//! blocks the service's sampler thread uses — `ProcessSampler::sample` →
//! [`build_groups`] → [`safety::apply`] — because the service publishes from its
//! own thread and exposes no per-tick timing. The assembly deliberately lives
//! here: this task's write set does not include `crates/memory`. `stdio` uses
//! the service itself, so every piece of live state stays in one place.

use anyhow::Result;
use clap::{Args, Subcommand};
use spacelens_memory::platform::{default_apps, default_sampler, default_signaller};
use spacelens_memory::sampler::{AppDirectory, Clock, ProcessSampler, Signaller, Sleeper};
use spacelens_memory::{
  serve_stdio, MemoryBreakdown, MemoryError, MemoryService, MemorySnapshot, PressureSource,
  ProcessRow, RawSystem, SnapshotNote,
};
use std::io::{self, BufReader};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The gap between the two samples `snapshot` takes, in milliseconds.
const SNAPSHOT_GAP_MS: u64 = 500;
/// The interval the one-shot `snapshot` reports for its single reading.
const SNAPSHOT_INTERVAL_MS: u32 = 500;
/// The interval `bench` reports while timing back-to-back ticks.
const BENCH_INTERVAL_MS: u32 = 1_000;
/// How long `stdio` waits without a call before it exits (§4.6).
const STDIO_IDLE_EXIT_MS: u64 = 120_000;

/// The `spacelens memory` subcommands.
#[derive(Debug, Subcommand)]
pub enum MemoryCommand {
  /// Print one memory snapshot as JSON.
  Snapshot(SnapshotArgs),
  /// Serve the stdio protocol the HTTP host speaks.
  Stdio,
  /// Time the sampler tick and print its percentiles as JSON.
  Bench(BenchArgs),
}

/// Arguments of `memory snapshot`.
#[derive(Debug, Args)]
pub struct SnapshotArgs {
  /// Print the snapshot as JSON instead of a short summary.
  #[arg(long)]
  pub json: bool,
}

/// Arguments of `memory bench`.
#[derive(Debug, Args)]
pub struct BenchArgs {
  /// How many samples to time.
  #[arg(long, default_value_t = 60)]
  pub ticks: u32,
}

/// Run the `memory` subcommand the caller selected.
pub fn run(command: MemoryCommand) -> Result<()> {
  match command {
    MemoryCommand::Snapshot(args) => run_snapshot(&args),
    MemoryCommand::Stdio => run_stdio(),
    MemoryCommand::Bench(args) => run_bench(&args),
  }
}

/// Print one snapshot taken after a 500 ms warm-up sample.
fn run_snapshot(args: &SnapshotArgs) -> Result<()> {
  let mut sampler = default_sampler();
  let mut directory = default_apps();
  let signaller = default_signaller();

  // The first reading has no previous tick to diff CPU usage against, so it is
  // discarded and the second is printed.
  let _warm_up = build_snapshot(
    &mut *sampler,
    &mut *directory,
    &*signaller,
    1,
    SNAPSHOT_INTERVAL_MS,
  )?;
  thread::sleep(Duration::from_millis(SNAPSHOT_GAP_MS));
  let snapshot = build_snapshot(
    &mut *sampler,
    &mut *directory,
    &*signaller,
    2,
    SNAPSHOT_INTERVAL_MS,
  )?;

  if args.json {
    print_json(&snapshot)?;
  } else {
    println!("sequence\t{}", snapshot.sequence);
    println!("platform\t{:?}", snapshot.platform);
    println!("sampled at\t{}", snapshot.sampled_at);
    println!(
      "used / total\t{} / {}",
      snapshot.breakdown.used_bytes, snapshot.breakdown.total_bytes
    );
    println!("pressure\t{:?}", snapshot.pressure.level);
    println!("groups\t{}", snapshot.groups.len());
    println!("processes\t{}", snapshot.processes.len());
  }
  Ok(())
}

/// Serve the stdio protocol on the calling (main) thread until `shutdown`, EOF
/// or the idle timeout.
fn run_stdio() -> Result<()> {
  let service = MemoryService::start(
    default_sampler(),
    default_apps(),
    default_signaller(),
    Arc::new(SystemClock),
    Arc::new(ThreadSleeper),
    None,
  );

  let input = BufReader::new(io::stdin());
  let output = io::stdout().lock();
  serve_stdio(&service, input, output, STDIO_IDLE_EXIT_MS)?;
  Ok(())
}

/// Time `ticks` sampler ticks and print `{p50Ms,p95Ms,maxMs,processes,rssBytes}`.
fn run_bench(args: &BenchArgs) -> Result<()> {
  if args.ticks == 0 {
    anyhow::bail!("bench requires at least one tick");
  }

  let mut sampler = default_sampler();
  let mut directory = default_apps();
  let signaller = default_signaller();
  let own_pid = signaller.own_pid();

  let mut durations: Vec<f64> = Vec::with_capacity(args.ticks as usize);
  let mut last: Option<MemorySnapshot> = None;
  for tick in 0..args.ticks {
    let started = Instant::now();
    let snapshot = build_snapshot(
      &mut *sampler,
      &mut *directory,
      &*signaller,
      u64::from(tick) + 1,
      BENCH_INTERVAL_MS,
    )?;
    durations.push(started.elapsed().as_secs_f64() * 1_000.0);
    last = Some(snapshot);
  }

  let snapshot = last.ok_or_else(|| anyhow::anyhow!("bench produced no snapshot"))?;
  durations.sort_by(f64::total_cmp);
  let report = serde_json::json!({
    "p50Ms": round_millis(percentile(&durations, 0.50)),
    "p95Ms": round_millis(percentile(&durations, 0.95)),
    "maxMs": round_millis(durations.last().copied().unwrap_or(0.0)),
    "processes": snapshot.processes.len(),
    "rssBytes": own_rss(&snapshot, own_pid).unwrap_or(0),
  });
  print_json(&report)?;
  Ok(())
}

/// Build one wire snapshot from a single sampler reading, exactly as
/// [`MemoryService`]'s sampler thread does for each tick.
fn build_snapshot(
  sampler: &mut dyn ProcessSampler,
  directory: &mut dyn AppDirectory,
  signaller: &dyn Signaller,
  sequence: u64,
  interval_ms: u32,
) -> Result<MemorySnapshot, MemoryError> {
  let platform = sampler.platform();
  let raw = sampler.sample()?;
  let running_apps = directory.apps();
  let frontmost = directory.frontmost_pid();
  let current_uid = signaller.current_uid();
  let own_pid = signaller.own_pid();

  let (mut groups, mut rows) =
    spacelens_memory::group::build_groups(&raw.processes, &running_apps, frontmost, current_uid);
  let safety = spacelens_memory::safety::context(&raw.processes, own_pid, current_uid, &groups);
  spacelens_memory::safety::apply(&mut groups, &mut rows, &raw.processes, &safety);

  let breakdown = breakdown(&raw.system);
  let notes = notes(&raw.system, &breakdown, &rows);
  Ok(MemorySnapshot {
    sequence,
    sampled_at: iso_from_ms(now_ms()),
    interval_ms,
    platform,
    breakdown,
    pressure: raw.system.pressure.clone(),
    groups,
    processes: rows,
    notes,
  })
}

/// The wire breakdown for one raw system reading, mirroring the service: the
/// macOS page counters give the full breakdown, everywhere else the portable
/// totals are used and the optional parts stay unavailable.
fn breakdown(system: &RawSystem) -> MemoryBreakdown {
  #[cfg(target_os = "macos")]
  if let Some(pages) = system.vm.as_ref() {
    return spacelens_memory::platform::macos::breakdown_from_pages(
      pages,
      system.total_bytes,
      (system.swap_total_bytes, system.swap_used_bytes),
    );
  }

  MemoryBreakdown {
    total_bytes: system.total_bytes,
    used_bytes: system.used_bytes,
    free_bytes: system.free_bytes,
    available_bytes: system.available_bytes,
    app_bytes: None,
    wired_bytes: None,
    compressed_bytes: None,
    cached_files_bytes: None,
    swap_total_bytes: system.swap_total_bytes,
    swap_used_bytes: system.swap_used_bytes,
  }
}

/// The caveats a one-shot snapshot carries, in enum order (the service's
/// `truncated` note does not apply here: nothing is capped).
fn notes(
  system: &RawSystem,
  breakdown: &MemoryBreakdown,
  rows: &[ProcessRow],
) -> Vec<SnapshotNote> {
  let mut notes = Vec::new();
  if !rows.is_empty() && rows.iter().all(|row| row.cpu_percent.is_none()) {
    notes.push(SnapshotNote::CpuWarmingUp);
  }
  if system.vm.is_some() && rows.iter().any(|row| row.restricted) {
    notes.push(SnapshotNote::RestrictedProcesses);
  }
  if breakdown.app_bytes.is_none() {
    notes.push(SnapshotNote::BreakdownUnavailable);
  }
  if system.pressure.source == PressureSource::Derived {
    notes.push(SnapshotNote::PressureDerived);
  }
  notes
}

/// The resident bytes of this process as reported in `snapshot`.
fn own_rss(snapshot: &MemorySnapshot, own_pid: u32) -> Option<u64> {
  snapshot
    .processes
    .iter()
    .find(|row| row.pid == own_pid)
    .and_then(|row| row.footprint_bytes.or(row.resident_bytes))
}

/// The nearest-rank percentile `fraction` of an ascending `sorted` slice.
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
  if sorted.is_empty() {
    return 0.0;
  }
  let rank = (fraction * sorted.len() as f64).ceil() as usize;
  let index = rank.saturating_sub(1).min(sorted.len() - 1);
  sorted[index]
}

/// Round a millisecond value to three decimals for a stable JSON report.
fn round_millis(value: f64) -> f64 {
  (value * 1_000.0).round() / 1_000.0
}

/// The current time in milliseconds since the Unix epoch.
fn now_ms() -> i64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
    .unwrap_or(0)
}

/// Render milliseconds since the Unix epoch as an ISO-8601 UTC instant with
/// millisecond precision (`2026-10-18T09:00:00.000Z`), matching `z.iso.datetime`.
/// Kept in step with the identical helper in `crates/memory/src/service.rs`.
fn iso_from_ms(now_ms: i64) -> String {
  let total_seconds = now_ms.div_euclid(1_000);
  let millis = now_ms.rem_euclid(1_000) as u32;
  let days = total_seconds.div_euclid(86_400);
  let second_of_day = total_seconds.rem_euclid(86_400);
  let (year, month, day) = civil_from_days(days);
  let hour = second_of_day / 3_600;
  let minute = (second_of_day % 3_600) / 60;
  let second = second_of_day % 60;
  format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Days since the Unix epoch → `(year, month, day)` (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
  let shifted = days + 719_468;
  let era = if shifted >= 0 {
    shifted
  } else {
    shifted - 146_096
  } / 146_097;
  let day_of_era = shifted - era * 146_097;
  let year_of_era =
    (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
  let year = year_of_era + era * 400;
  let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
  let month_prime = (5 * day_of_year + 2) / 153;
  let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
  let month = (if month_prime < 10 {
    month_prime + 3
  } else {
    month_prime - 9
  }) as u32;
  let year = if month <= 2 { year + 1 } else { year };
  (year, month, day)
}

/// Serialize `value` pretty-printed to stdout.
fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
  serde_json::to_writer_pretty(io::stdout(), value)?;
  println!();
  Ok(())
}

/// The wall clock the stdio service paces its sampler thread with.
struct SystemClock;

impl Clock for SystemClock {
  fn now_ms(&self) -> i64 {
    now_ms()
  }
}

/// The sleeper the stdio service paces its sampler thread with.
struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
  fn sleep_ms(&self, ms: u64) {
    thread::sleep(Duration::from_millis(ms));
  }
}
