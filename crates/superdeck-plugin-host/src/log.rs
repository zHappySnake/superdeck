// SPDX-License-Identifier: GPL-3.0-or-later

//! Plugin log files: `<logs>/<uuid>.log`, with the previous run's kept as
//! `<uuid>.log.1`.

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// A log stops growing at this size.
const MAX_SIZE: u64 = 10 * 1024 * 1024;

/// Where a log line comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// SuperDeck's notes about the plugin.
    Host,
    /// The plugin's standard output.
    Stdout,
    /// The plugin's standard error.
    Stderr,
    /// `logMessage` from the plugin.
    Plugin,
    /// `logMessage` from a property inspector.
    Inspector,
}

impl Source {
    fn tag(self) -> &'static str {
        match self {
            Source::Host => "superdeck",
            Source::Stdout => "stdout",
            Source::Stderr => "stderr",
            Source::Plugin => "plugin",
            Source::Inspector => "inspector",
        }
    }
}

/// A plugin's log; cheap to clone, and shared by everything that writes to
/// it.
#[derive(Debug, Clone)]
pub(crate) struct PluginLog {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug)]
struct Inner {
    path: PathBuf,
    file: Option<File>,
    written: u64,
}

impl PluginLog {
    /// Starts the log of a run of the host, keeping the previous one.
    pub fn open(dir: &Path, uuid: &str) -> (Self, Option<io::Error>) {
        let path = dir.join(format!("{uuid}.log"));
        let mut previous = path.clone().into_os_string();
        previous.push(".1");
        let opened = fs::create_dir_all(dir)
            .and_then(|()| match fs::rename(&path, &previous) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            })
            .and_then(|()| {
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&path)
            });
        let (file, error) = match opened {
            Ok(file) => (Some(file), None),
            Err(e) => (None, Some(e)),
        };
        let log = Self {
            inner: Arc::new(Mutex::new(Inner {
                path,
                file,
                written: 0,
            })),
        };
        (log, error)
    }

    /// A log that goes nowhere, for tests.
    #[cfg(test)]
    pub fn discard() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                path: PathBuf::new(),
                file: None,
                written: 0,
            })),
        }
    }

    /// The log file.
    pub fn path(&self) -> PathBuf {
        self.lock().path.clone()
    }

    /// Writes a line, or several, each with the time and `source`.
    pub fn write(&self, source: Source, text: &str) {
        let mut inner = self.lock();
        let inner = &mut *inner;
        let Some(file) = &mut inner.file else { return };
        let mut lines = String::new();
        let time = timestamp(SystemTime::now());
        for line in text.trim_end_matches(['\r', '\n']).split('\n') {
            let line = line.trim_end_matches('\r');
            let _ = writeln!(lines, "{time} [{}] {line}", source.tag());
        }
        let size = lines.len() as u64;
        if inner.written + size > MAX_SIZE {
            let note = format!("{time} [superdeck] the log is full; nothing more is written\n");
            let _ = file.write_all(note.as_bytes());
            inner.file = None;
            return;
        }
        if file.write_all(lines.as_bytes()).is_err() {
            inner.file = None;
        }
        inner.written += size;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `2026-10-10T12:34:56.789Z`.
fn timestamp(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = since.as_secs();
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let (year, month, day) = civil_from_days(days);
    let of_day = seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        of_day / 3600,
        of_day / 60 % 60,
        of_day % 60,
        since.subsec_millis()
    )
}

/// The date `days` after 1970-01-01, by Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).expect("1 to 31");
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).expect("1 to 12");
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
        let time = UNIX_EPOCH + Duration::from_millis(1_791_635_696_789);
        assert_eq!(timestamp(time), "2026-10-10T12:34:56.789Z");
        let leap = UNIX_EPOCH + Duration::from_hours(11_016 * 24);
        assert_eq!(timestamp(leap), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn logs_rotate_and_stop_growing() {
        let temp = tempfile::tempdir().unwrap();
        let (log, error) = PluginLog::open(temp.path(), "com.example.a");
        assert!(error.is_none());
        log.write(Source::Stdout, "one\r\ntwo\n");
        let text = fs::read_to_string(log.path()).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with(" [stdout] one"), "{}", lines[0]);
        assert!(lines[1].ends_with(" [stdout] two"), "{}", lines[1]);

        let (log, _) = PluginLog::open(temp.path(), "com.example.a");
        assert_eq!(
            fs::read_to_string(temp.path().join("com.example.a.log.1")).unwrap(),
            text
        );
        let big = "x".repeat(1024 * 1024);
        for _ in 0..12 {
            log.write(Source::Plugin, &big);
        }
        let size = fs::metadata(log.path()).unwrap().len();
        assert!(size <= MAX_SIZE + 100, "{size}");
        assert!(
            fs::read_to_string(log.path())
                .unwrap()
                .ends_with("the log is full; nothing more is written\n")
        );
    }
}
