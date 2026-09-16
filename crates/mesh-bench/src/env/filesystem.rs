//! Which filesystem the workload data actually sat on.
//!
//! Mesh's numbers are storage numbers. The same code on APFS, ext4 and tmpfs is
//! three different benchmarks, so the filesystem type is required metadata and
//! is resolved by longest-prefix match against the real mount table rather than
//! assumed from the path.

use super::exec::output;
use super::ProbeError;
use std::path::Path;

/// One row of the host's mount table.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MountPoint {
    mount_path: String,
    filesystem: String,
}

/// Resolves the filesystem type backing `path`.
pub fn probe_filesystem(path: &Path) -> Result<String, ProbeError> {
    let absolute = path.canonicalize().map_err(|error| {
        ProbeError::missing(
            "platform.filesystem",
            format!("cannot canonicalise {}: {error}", path.display()),
        )
    })?;
    let table = read_mount_table()?;
    resolve(&table, &absolute.to_string_lossy()).ok_or_else(|| {
        ProbeError::missing(
            "platform.filesystem",
            format!("no mount point covers {}", absolute.display()),
        )
    })
}

fn read_mount_table() -> Result<Vec<MountPoint>, ProbeError> {
    if Path::new("/proc/self/mounts").exists() {
        let text = std::fs::read_to_string("/proc/self/mounts").map_err(|error| {
            ProbeError::missing("platform.filesystem", format!("/proc/self/mounts: {error}"))
        })?;
        return Ok(parse_proc_mounts(&text));
    }
    let text = output("mount", &[])?;
    let table = parse_bsd_mount(&text);
    if table.is_empty() {
        return Err(ProbeError::missing(
            "platform.filesystem",
            "`mount` produced no parseable rows",
        ));
    }
    Ok(table)
}

/// Parses `/proc/self/mounts`: `device mountpoint fstype options …`.
fn parse_proc_mounts(text: &str) -> Vec<MountPoint> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let _device = fields.next()?;
            let mount_path = unescape_octal(fields.next()?);
            let filesystem = fields.next()?.to_owned();
            Some(MountPoint {
                mount_path,
                filesystem,
            })
        })
        .collect()
}

/// Parses BSD `mount`: `/dev/disk3s5 on / (apfs, local, journaled)`.
fn parse_bsd_mount(text: &str) -> Vec<MountPoint> {
    text.lines()
        .filter_map(|line| {
            let (_device, rest) = line.split_once(" on ")?;
            let (mount_path, attributes) = rest.rsplit_once(" (")?;
            let filesystem = attributes
                .trim_end_matches(')')
                .split(',')
                .next()?
                .trim()
                .to_owned();
            if filesystem.is_empty() {
                return None;
            }
            Some(MountPoint {
                mount_path: mount_path.to_owned(),
                filesystem,
            })
        })
        .collect()
}

/// `/proc` escapes spaces and tabs in mount paths as octal.
fn unescape_octal(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        let digits: String = chars.clone().take(3).collect();
        match u32::from_str_radix(&digits, 8)
            .ok()
            .and_then(char::from_u32)
        {
            Some(decoded) if digits.len() == 3 => {
                out.push(decoded);
                for _ in 0..3 {
                    chars.next();
                }
            }
            _ => out.push(character),
        }
    }
    out
}

/// Longest mount path that is a prefix of `path` wins — the nested mount, not
/// the root one.
fn resolve(table: &[MountPoint], path: &str) -> Option<String> {
    table
        .iter()
        .filter(|mount| covers(&mount.mount_path, path))
        .max_by_key(|mount| mount.mount_path.len())
        .map(|mount| mount.filesystem.clone())
}

fn covers(mount_path: &str, path: &str) -> bool {
    if mount_path == "/" {
        return path.starts_with('/');
    }
    path == mount_path
        || path
            .strip_prefix(mount_path)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BSD: &str = "/dev/disk3s1s1 on / (apfs, sealed, local, read-only, journaled)\n\
         devfs on /dev (devfs, local, nobrowse)\n\
         /dev/disk3s5 on /System/Volumes/Data (apfs, local, journaled, nobrowse)\n\
         map auto_home on /System/Volumes/Data/home (autofs, automounted, nobrowse)";

    const PROC: &str = "/dev/root / ext4 rw,relatime 0 0\n\
         proc /proc proc rw,nosuid 0 0\n\
         tmpfs /tmp tmpfs rw,nosuid,nodev 0 0\n\
         /dev/nvme0n1p2 /var/lib/mesh\\040data xfs rw 0 0";

    #[test]
    fn bsd_rows_yield_mount_points() {
        let table = parse_bsd_mount(BSD);
        assert_eq!(table.len(), 4);
        assert_eq!(resolve(&table, "/Users/x/repo").as_deref(), Some("apfs"));
        assert_eq!(resolve(&table, "/dev/null").as_deref(), Some("devfs"));
    }

    #[test]
    fn proc_rows_yield_mount_points() {
        let table = parse_proc_mounts(PROC);
        assert_eq!(resolve(&table, "/home/x/repo").as_deref(), Some("ext4"));
        assert_eq!(resolve(&table, "/tmp/bench").as_deref(), Some("tmpfs"));
    }

    #[test]
    fn the_deepest_mount_wins() {
        let table = parse_bsd_mount(BSD);
        assert_eq!(
            resolve(&table, "/System/Volumes/Data/home/x").as_deref(),
            Some("autofs"),
            "a nested mount must beat the root mount"
        );
    }

    #[test]
    fn prefixes_must_land_on_a_path_boundary() {
        let table = parse_proc_mounts(PROC);
        assert_eq!(resolve(&table, "/tmpfoo/bar").as_deref(), Some("ext4"));
    }

    #[test]
    fn octal_escapes_in_mount_paths_are_decoded() {
        let table = parse_proc_mounts(PROC);
        assert_eq!(
            resolve(&table, "/var/lib/mesh data/blobs").as_deref(),
            Some("xfs")
        );
    }

    #[test]
    fn the_probe_resolves_a_real_directory() {
        let filesystem = probe_filesystem(Path::new(".")).expect("the cwd is on a filesystem");
        assert!(!filesystem.is_empty());
    }

    #[test]
    fn a_missing_directory_is_a_named_failure() {
        let error = probe_filesystem(Path::new("/mesh-bench/definitely/not/here"))
            .expect_err("missing paths have no filesystem");
        assert!(error.to_string().contains("platform.filesystem"));
    }
}
