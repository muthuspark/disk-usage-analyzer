use crate::model::Node;
use rayon::prelude::*;
use std::{
    collections::HashSet,
    fs, io,
    os::unix::fs::MetadataExt,
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Instant, SystemTime},
};

#[derive(Default)]
pub struct Progress {
    pub entries: AtomicU64,
    pub errors: AtomicU64,
    pub skipped_mounts: AtomicU64,
    pub cancel: AtomicBool,
}

pub struct Report {
    pub root: Node,
    pub seconds: f64,
}

struct Scanner<'a> {
    device: u64,
    hardlinks: Vec<Mutex<HashSet<(u64, u64)>>>,
    progress: &'a Progress,
}

pub fn scan(path: &Path, label: String, progress: &Progress) -> io::Result<Report> {
    let threads = std::env::var("DISK_SCAN_THREADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get().min(8)));
    scan_with_threads(path, label, progress, threads)
}

fn scan_with_threads(
    path: &Path,
    label: String,
    progress: &Progress,
    threads: usize,
) -> io::Result<Report> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose a directory to scan",
        ));
    }
    let started = Instant::now();
    let scanner = Scanner {
        device: metadata.dev(),
        hardlinks: (0..32).map(|_| Mutex::new(HashSet::new())).collect(),
        progress,
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(io::Error::other)?;
    let (mut root, _) = pool.install(|| scanner.directory(path, &metadata, 0))?;
    root.name = label;
    Ok(Report {
        root,
        seconds: started.elapsed().as_secs_f64(),
    })
}

impl Scanner<'_> {
    fn directory(
        &self,
        path: &Path,
        metadata: &fs::Metadata,
        depth: usize,
    ) -> io::Result<(Node, Option<SystemTime>)> {
        if self.progress.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"));
        }
        let mut node = Node::empty(
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
        );
        node.bytes = metadata.blocks() * 512;
        let mut latest = metadata.modified().ok();
        node.modified = modified_age(latest);
        let entries = match fs::read_dir(path) {
            Ok(entries) if depth < 512 => entries,
            _ => {
                node.restricted = true;
                self.progress.errors.fetch_add(1, Ordering::Relaxed);
                return Ok((node, latest));
            }
        };
        let mut children = vec![];
        let mut directories = vec![];
        for entry in entries {
            if self.progress.cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"));
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    self.progress.errors.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
            // symlink_metadata deliberately avoids following symlinks or their cycles.
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(_) => {
                    self.progress.errors.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
            };
            if metadata.dev() != self.device {
                self.progress.skipped_mounts.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            self.progress.entries.fetch_add(1, Ordering::Relaxed);
            node.items += 1;
            latest = latest.max(metadata.modified().ok());
            if metadata.is_dir() {
                directories.push((entry.path(), metadata));
            } else {
                // Multiple directory entries for one hard-linked file occupy space once.
                if metadata.nlink() > 1
                    && !self.hardlinks[metadata.ino() as usize % self.hardlinks.len()]
                        .lock()
                        .unwrap()
                        .insert((metadata.dev(), metadata.ino()))
                {
                    continue;
                }
                node.bytes += metadata.blocks() * 512;
                if metadata.file_type().is_symlink() {
                    let mut link = Node::empty(entry.file_name().to_string_lossy().into());
                    link.bytes = metadata.blocks() * 512;
                    link.modified = modified_age(metadata.modified().ok());
                    children.push(link);
                }
            }
        }
        let subdirectories: io::Result<Vec<_>> = directories
            .into_par_iter()
            .map(|(path, metadata)| self.directory(&path, &metadata, depth + 1))
            .collect();
        for (child, child_latest) in subdirectories? {
            node.bytes += child.bytes;
            node.items += child.items;
            latest = latest.max(child_latest);
            children.push(child);
        }
        children.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
        node.children = children.into();
        node.modified = modified_age(latest);
        Ok((node, latest))
    }
}

fn modified_age(time: Option<SystemTime>) -> String {
    let Some(time) = time else {
        return "—".into();
    };
    let days = SystemTime::now()
        .duration_since(time)
        .unwrap_or_default()
        .as_secs()
        / 86400;
    match days {
        0 => "Today".into(),
        1 => "1 day".into(),
        2..30 => format!("{days} days"),
        30..60 => "1 month".into(),
        60..365 => format!("{} months", days / 30),
        365..730 => "1 year".into(),
        _ => format!("{} years", days / 365),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn totals_match_du_without_following_symlinks_or_double_counting_hardlinks() {
        let path = std::env::temp_dir().join(format!("gpui-scan-test-{}", std::process::id()));
        fs::create_dir_all(path.join("nested")).unwrap();
        fs::write(path.join("nested/file"), vec![1_u8; 8192]).unwrap();
        fs::hard_link(path.join("nested/file"), path.join("hardlink")).unwrap();
        symlink(&path, path.join("cycle")).unwrap();
        let report = scan(&path, "test".into(), &Progress::default()).unwrap();
        let serial = scan_with_threads(&path, "test".into(), &Progress::default(), 1).unwrap();
        let parallel = scan_with_threads(&path, "test".into(), &Progress::default(), 8).unwrap();
        assert_eq!(serial.root.bytes, parallel.root.bytes);
        assert_eq!(serial.root.items, parallel.root.items);
        let output = std::process::Command::new("du")
            .args(["-s", "-B1", "-x"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(output.status.success());
        let du_bytes: u64 = String::from_utf8(output.stdout)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(report.root.bytes, du_bytes);
        assert_eq!(report.root.items, 4);
        assert_eq!(report.root.children.len(), 2);
        assert!(report.root.children.iter().any(|n| n.name == "nested"));
        assert!(
            report
                .root
                .children
                .iter()
                .any(|n| n.name == "cycle" && n.children.is_empty())
        );
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn missing_paths_and_cancellation_are_errors() {
        let progress = Progress::default();
        progress.cancel.store(true, Ordering::Relaxed);
        assert!(scan(Path::new("/"), "root".into(), &progress).is_err());
        assert!(
            scan(
                Path::new("/does-not-exist-gpui-test"),
                "missing".into(),
                &Progress::default()
            )
            .is_err()
        );
    }

    #[test]
    fn unreadable_directories_are_reported_as_partial_results() {
        use std::os::unix::fs::PermissionsExt;
        let path =
            std::env::temp_dir().join(format!("gpui-scan-permissions-{}", std::process::id()));
        fs::create_dir_all(path.join("blocked")).unwrap();
        fs::set_permissions(path.join("blocked"), fs::Permissions::from_mode(0o000)).unwrap();
        let progress = Progress::default();
        let report = scan(&path, "test".into(), &progress).unwrap();
        let blocked = fs::read_dir(path.join("blocked")).is_err();
        fs::set_permissions(path.join("blocked"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::remove_dir_all(path).unwrap();
        // An elevated test process can read mode-000 directories.
        if blocked {
            assert!(report.root.children[0].restricted);
            assert_eq!(progress.errors.load(Ordering::Relaxed), 1);
        }
    }
}
