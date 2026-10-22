//! macOS facts from `diskutil` (plist output) and Foundation.
//!
//! `diskutil` runs by absolute path with a timeout, so a missing or wedged
//! tool degrades to "no breakdown" instead of stalling a finished scan.

use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::{ApfsVolumeUsage, ScannedRoot, SnapshotSummary, StorageBreakdown};

const DISKUTIL: &str = "/usr/sbin/diskutil";
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct DiskInfo {
  #[serde(rename = "APFSContainerReference")]
  container: Option<String>,
}

#[derive(Deserialize)]
struct ApfsList {
  #[serde(rename = "Containers", default)]
  containers: Vec<Container>,
}

#[derive(Deserialize)]
struct Container {
  #[serde(rename = "ContainerReference")]
  reference: String,
  #[serde(rename = "CapacityCeiling")]
  capacity: u64,
  #[serde(rename = "CapacityFree")]
  free: u64,
  #[serde(rename = "Volumes", default)]
  volumes: Vec<Volume>,
}

#[derive(Deserialize)]
struct Volume {
  #[serde(rename = "DeviceIdentifier")]
  device: String,
  #[serde(rename = "Name", default)]
  name: String,
  #[serde(rename = "CapacityInUse", default)]
  in_use: u64,
  #[serde(rename = "Roles", default)]
  roles: Vec<String>,
}

#[derive(Deserialize)]
struct SnapshotList {
  #[serde(rename = "Snapshots", default)]
  snapshots: Vec<Snapshot>,
}

#[derive(Deserialize)]
struct Snapshot {
  #[serde(rename = "SnapshotName", default)]
  name: String,
}

pub(crate) fn storage_breakdown(roots: &[ScannedRoot]) -> Option<StorageBreakdown> {
  // Group scanned bytes by container. `diskutil info` only answers for a
  // device or mount point, so each root is first mapped to its mount; a
  // firmlinked path such as `/Users/me` resolves to the Data volume.
  let mut by_container: BTreeMap<String, (u64, &Path)> = BTreeMap::new();
  for root in roots {
    let Some(container) = mount_point(&root.path)
      .and_then(|mount| diskutil(&["info", "-plist"], Some(&mount)))
      .and_then(|bytes| parse_container_reference(&bytes))
    else {
      continue;
    };
    let entry = by_container.entry(container).or_insert((0, &root.path));
    entry.0 = entry.0.saturating_add(root.bytes);
  }
  let (container, (scanned_bytes, probe)) = by_container
    .into_iter()
    .max_by_key(|(_, (bytes, _))| *bytes)?;
  let list = diskutil(&["apfs", "list", "-plist"], None)?;
  let mut breakdown = parse_container(&list, &container, scanned_bytes)?;
  let snapshot_volume = snapshot_volume(&breakdown.volumes);
  breakdown.snapshots = snapshot_volume.and_then(|device| {
    diskutil(&["apfs", "listSnapshots", "-plist", &device], None)
      .and_then(|bytes| parse_snapshots(&bytes, &device))
  });
  breakdown.purgeable_bytes = purgeable_bytes(probe);
  Some(breakdown)
}

/// The mount point of the filesystem holding `path`, from `statfs`.
fn mount_point(path: &Path) -> Option<PathBuf> {
  let path = CString::new(path.as_os_str().as_bytes()).ok()?;
  let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
  // SAFETY: `path` is NUL-terminated and `stats` is written before it is read.
  if unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
    return None;
  }
  let stats = unsafe { stats.assume_init() };
  // SAFETY: the kernel NUL-terminates `f_mntonname` within its buffer.
  let mount = unsafe { CStr::from_ptr(stats.f_mntonname.as_ptr()) };
  Some(PathBuf::from(std::ffi::OsStr::from_bytes(mount.to_bytes())))
}

fn parse_container_reference(bytes: &[u8]) -> Option<String> {
  plist::from_bytes::<DiskInfo>(bytes).ok()?.container
}

fn parse_container(bytes: &[u8], reference: &str, scanned_bytes: u64) -> Option<StorageBreakdown> {
  let list: ApfsList = plist::from_bytes(bytes).ok()?;
  let container = list
    .containers
    .into_iter()
    .find(|container| container.reference == reference)?;
  let mut volumes: Vec<ApfsVolumeUsage> = container
    .volumes
    .into_iter()
    .map(|volume| ApfsVolumeUsage {
      name: volume.name,
      role: volume
        .roles
        .into_iter()
        .next()
        .unwrap_or_else(|| "None".into()),
      device: volume.device,
      in_use_bytes: volume.in_use,
    })
    .collect();
  volumes.sort_by(|a, b| {
    b.in_use_bytes
      .cmp(&a.in_use_bytes)
      .then_with(|| a.device.cmp(&b.device))
  });
  Some(StorageBreakdown {
    container: container.reference,
    capacity_bytes: container.capacity,
    in_use_bytes: container.capacity.saturating_sub(container.free),
    purgeable_bytes: None,
    scanned_bytes,
    volumes,
    snapshots: None,
  })
}

/// Snapshots that hold deleted user data live on the Data volume; a
/// container without one (an external APFS disk) uses its largest volume.
fn snapshot_volume(volumes: &[ApfsVolumeUsage]) -> Option<String> {
  volumes
    .iter()
    .find(|volume| volume.role == "Data")
    .or_else(|| volumes.first())
    .map(|volume| volume.device.clone())
}

fn parse_snapshots(bytes: &[u8], device: &str) -> Option<SnapshotSummary> {
  let list: SnapshotList = plist::from_bytes(bytes).ok()?;
  let stamps: Vec<String> = list
    .snapshots
    .iter()
    .filter_map(|snapshot| time_machine_stamp(&snapshot.name))
    .collect();
  Some(SnapshotSummary {
    volume: device.into(),
    count: u32::try_from(list.snapshots.len()).unwrap_or(u32::MAX),
    time_machine_count: u32::try_from(stamps.len()).unwrap_or(u32::MAX),
    oldest_time_machine: stamps.into_iter().min(),
  })
}

/// `com.apple.TimeMachine.2026-10-05-160111.local` -> `2026-10-05T16:01:11`.
fn time_machine_stamp(name: &str) -> Option<String> {
  let stamp = name
    .strip_prefix("com.apple.TimeMachine.")?
    .strip_suffix(".local")?;
  let (date, time) = stamp.split_at_checked(10)?;
  let time = time.strip_prefix('-')?;
  let valid = date.len() == 10
    && date.bytes().enumerate().all(|(index, byte)| {
      if index == 4 || index == 7 {
        byte == b'-'
      } else {
        byte.is_ascii_digit()
      }
    })
    && time.len() == 6
    && time.bytes().all(|byte| byte.is_ascii_digit());
  valid.then(|| format!("{date}T{}:{}:{}", &time[0..2], &time[2..4], &time[4..6]))
}

/// Foundation's "available for important usage" counts purgeable space as
/// free; plain "available" does not. The difference is what macOS can
/// reclaim on demand.
fn purgeable_bytes(path: &Path) -> Option<u64> {
  use objc2::rc::autoreleasepool;
  use objc2_foundation::{
    NSArray, NSNumber, NSString, NSURLVolumeAvailableCapacityForImportantUsageKey,
    NSURLVolumeAvailableCapacityKey, NSURL,
  };
  autoreleasepool(|_| {
    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
    // SAFETY: both keys are immutable Foundation constants.
    let (important_key, available_key) = unsafe {
      (
        NSURLVolumeAvailableCapacityForImportantUsageKey,
        NSURLVolumeAvailableCapacityKey,
      )
    };
    let values = url
      .resourceValuesForKeys_error(&NSArray::from_slice(&[important_key, available_key]))
      .ok()?;
    let number = |key| {
      let value = values.objectForKey(key)?.downcast::<NSNumber>().ok()?;
      u64::try_from(value.longLongValue()).ok()
    };
    Some(number(important_key)?.saturating_sub(number(available_key)?))
  })
}

/// Run `diskutil` with an optional trailing path; `None` on spawn failure,
/// non-zero exit or timeout.
fn diskutil(args: &[&str], path: Option<&Path>) -> Option<Vec<u8>> {
  let mut command = Command::new(DISKUTIL);
  command.args(args);
  if let Some(path) = path {
    command.arg(path);
  }
  let mut child = command
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .ok()?;
  // Drain stdout on its own thread so a large plist cannot fill the pipe
  // while this thread waits on the exit status.
  let mut stdout = child.stdout.take()?;
  let reader = std::thread::spawn(move || {
    let mut bytes = Vec::new();
    stdout.read_to_end(&mut bytes).map(|_| bytes)
  });
  let started = Instant::now();
  let status = loop {
    match child.try_wait() {
      Ok(Some(status)) => break status,
      Ok(None) if started.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(20)),
      _ => {
        let _ = child.kill();
        let _ = child.wait();
        return None;
      }
    }
  };
  let bytes = reader.join().ok()?.ok()?;
  status.success().then_some(bytes)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn plist(body: &str) -> Vec<u8> {
    format!(
      r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">{body}</plist>"#
    )
    .into_bytes()
  }

  fn volume(device: &str, name: &str, role: Option<&str>, in_use: u64) -> String {
    let roles = role.map_or(String::new(), |role| format!("<string>{role}</string>"));
    format!(
      "<dict><key>DeviceIdentifier</key><string>{device}</string><key>Name</key><string>{name}</string>\
       <key>CapacityInUse</key><integer>{in_use}</integer><key>Roles</key><array>{roles}</array></dict>"
    )
  }

  fn apfs_list() -> Vec<u8> {
    plist(&format!(
      "<dict><key>Containers</key><array>\
       <dict><key>ContainerReference</key><string>disk1</string><key>CapacityCeiling</key><integer>500</integer>\
       <key>CapacityFree</key><integer>400</integer><key>Volumes</key><array>{}</array></dict>\
       <dict><key>ContainerReference</key><string>disk3</string><key>CapacityCeiling</key><integer>1000</integer>\
       <key>CapacityFree</key><integer>480</integer><key>Volumes</key><array>{}{}{}{}</array></dict>\
       </array></dict>",
      volume("disk1s1", "iSCPreboot", Some("Preboot"), 100),
      volume("disk3s1", "Macintosh HD", Some("System"), 14),
      volume("disk3s5", "Macintosh HD - Data", Some("Data"), 459),
      volume("disk3s6", "VM", Some("VM"), 30),
      volume("disk3s9", "Scratch", None, 17),
    ))
  }

  #[test]
  fn container_reference_comes_from_disk_info() {
    let info = plist("<dict><key>APFSContainerReference</key><string>disk3</string></dict>");
    assert_eq!(parse_container_reference(&info).as_deref(), Some("disk3"));
    let not_apfs = plist("<dict><key>FilesystemType</key><string>exfat</string></dict>");
    assert_eq!(parse_container_reference(&not_apfs), None);
  }

  #[test]
  fn container_reports_in_use_and_volumes_largest_first() {
    let breakdown = parse_container(&apfs_list(), "disk3", 287).unwrap();
    assert_eq!(breakdown.capacity_bytes, 1000);
    assert_eq!(breakdown.in_use_bytes, 520);
    assert_eq!(breakdown.scanned_bytes, 287);
    let roles: Vec<_> = breakdown
      .volumes
      .iter()
      .map(|volume| volume.role.as_str())
      .collect();
    assert_eq!(roles, ["Data", "VM", "None", "System"]);
    assert_eq!(
      snapshot_volume(&breakdown.volumes).as_deref(),
      Some("disk3s5")
    );
    assert!(parse_container(&apfs_list(), "disk9", 0).is_none());
    assert!(parse_container(b"not a plist", "disk3", 0).is_none());
  }

  #[test]
  fn snapshots_are_counted_with_the_oldest_time_machine_stamp() {
    let names = [
      "com.apple.TimeMachine.2026-10-21-204248.local",
      "com.apple.TimeMachine.2026-10-05-160111.local",
      "com.apple.os.update-9A91EDBD",
      "com.apple.TimeMachine.garbage.local",
    ];
    let body: String = names
      .iter()
      .map(|name| format!("<dict><key>SnapshotName</key><string>{name}</string></dict>"))
      .collect();
    let summary = parse_snapshots(
      &plist(&format!(
        "<dict><key>Snapshots</key><array>{body}</array></dict>"
      )),
      "disk3s5",
    )
    .unwrap();
    assert_eq!(
      summary,
      SnapshotSummary {
        volume: "disk3s5".into(),
        count: 4,
        time_machine_count: 2,
        oldest_time_machine: Some("2026-10-05T16:01:11".into()),
      }
    );
    let empty = parse_snapshots(
      &plist("<dict><key>Snapshots</key><array/></dict>"),
      "disk3s5",
    )
    .unwrap();
    assert_eq!((empty.count, empty.oldest_time_machine), (0, None));
  }

  #[test]
  fn breakdown_serializes_in_contract_shape() {
    let mut breakdown = parse_container(&apfs_list(), "disk3", 287).unwrap();
    breakdown.purgeable_bytes = Some(119);
    let json = serde_json::to_value(&breakdown).unwrap();
    assert_eq!(json["inUseBytes"], 520);
    assert_eq!(json["purgeableBytes"], 119);
    assert_eq!(json["volumes"][0]["inUseBytes"], 459);
    assert!(json["snapshots"].is_null());
  }

  #[test]
  fn folders_resolve_to_their_mount_point() {
    assert_eq!(mount_point(Path::new("/")).as_deref(), Some(Path::new("/")));
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let mount = mount_point(&home).unwrap();
    assert!(home.starts_with(&mount) || mount == Path::new("/System/Volumes/Data"));
    assert!(mount_point(Path::new("/definitely/not/here")).is_none());
  }

  /// A scan of a plain folder (not a mount point) still finds its container.
  #[test]
  fn home_folder_has_a_breakdown() {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    let breakdown = storage_breakdown(&[ScannedRoot {
      path: home,
      bytes: 7,
    }])
    .unwrap();
    assert_eq!(breakdown.scanned_bytes, 7);
  }

  /// The live probe on this machine: the startup disk is always APFS.
  #[test]
  fn startup_disk_has_a_breakdown() {
    let breakdown = storage_breakdown(&[ScannedRoot {
      path: "/".into(),
      bytes: 42,
    }])
    .unwrap();
    assert_eq!(breakdown.scanned_bytes, 42);
    assert!(breakdown.in_use_bytes > 0 && breakdown.in_use_bytes <= breakdown.capacity_bytes);
    assert!(breakdown.volumes.iter().any(|volume| volume.role == "Data"));
  }
}
