use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mycelia_core::SearchHeader;

const LOG_CAP_BYTES: u64 = 10 * 1024 * 1024; // 10 MB before rotation

/// Per-corpus append log at `~/.local/share/mycelia/logs/<name>.log`.
/// All writes are best-effort: I/O failures are silently ignored so log
/// problems never surface as command errors or interrupt MCP tool handling.
#[derive(Clone)]
pub(crate) struct CorpusLogger {
    path: PathBuf,
    corpus_root: Option<PathBuf>,
    inner: Arc<Mutex<Option<File>>>,
}

impl CorpusLogger {
    pub(crate) fn open(log_path: PathBuf, corpus_root: Option<PathBuf>) -> Self {
        let file = open_log_file(&log_path);
        Self {
            path: log_path,
            corpus_root,
            inner: Arc::new(Mutex::new(file)),
        }
    }

    pub(crate) fn log_serve_start(&self, model: &str, embeddings: &str) {
        self.write_line(format!(
            "{}  serve start  model={}  embeddings={}",
            now(),
            model,
            embeddings
        ));
    }

    pub(crate) fn log_find(&self, query: &str, headers: &[SearchHeader]) {
        let actual_tok = headers.iter().map(|h| h.approximate_bytes()).sum::<usize>() / 4;
        let results = headers.len();
        let line = match cold_tokens(headers, self.corpus_root.as_deref()) {
            Some(cold_tok) if cold_tok > 0 => {
                let ratio = if actual_tok > 0 {
                    cold_tok as f64 / actual_tok as f64
                } else {
                    1.0
                };
                format!(
                    "{}  find         q={:?}  results={}  actual_tok={}  cold_tok={}  ~{:.1}x saved",
                    now(),
                    query,
                    results,
                    actual_tok,
                    cold_tok,
                    ratio
                )
            }
            _ => format!(
                "{}  find         q={:?}  results={}  actual_tok={}",
                now(),
                query,
                results,
                actual_tok
            ),
        };
        self.write_line(line);
    }

    pub(crate) fn log_retrieve(&self, chunk_id: &str, source_path: &str) {
        self.write_line(format!(
            "{}  retrieve     chunk={}  path={}",
            now(),
            chunk_id,
            source_path
        ));
    }

    pub(crate) fn log_find_related(&self, symbol: &str, direction: &str, results: usize) {
        self.write_line(format!(
            "{}  find_related symbol={symbol:?}  direction={direction}  results={results}",
            now(),
        ));
    }

    pub(crate) fn log_find_changed(&self, paths_count: usize, results: usize) {
        self.write_line(format!(
            "{}  find_changed paths={paths_count}  results={results}",
            now(),
        ));
    }

    pub(crate) fn log_list_corpora(&self, count: usize) {
        self.write_line(format!("{}  list_corpora count={count}", now()));
    }

    /// Records a failed tool call so the log is a true audit of every invocation,
    /// not only the ones that succeeded. `detail` is the call's identifying
    /// argument (for example `q="..."` or `chunk=...`).
    pub(crate) fn log_error(&self, tool: &str, detail: &str, message: &str) {
        self.write_line(format!(
            "{}  {tool:<12} {detail}  status=error  msg={message:?}",
            now(),
        ));
    }

    fn write_line(&self, line: String) {
        let Ok(mut guard) = self.inner.lock() else {
            return;
        };
        // Rotate when the file exceeds the cap: rename to .log.bak, open fresh.
        if let Some(file) = guard.as_ref() {
            let over_cap = file.metadata().map(|m| m.len()).unwrap_or(0) > LOG_CAP_BYTES;
            if over_cap {
                let backup = self.path.with_extension("log.bak");
                let _ = fs::rename(&self.path, &backup);
                *guard = open_log_file(&self.path);
            }
        }
        if let Some(file) = guard.as_mut() {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}

// Stats reader

#[derive(Default)]
pub(crate) struct StatsReport {
    pub(crate) queries: u64,
    pub(crate) actual_tokens: u64,
    pub(crate) cold_tokens: u64,
    pub(crate) has_cold: bool,
}

/// Reads the last `limit` find/retrieve events from the corpus log.
pub(crate) fn recent_events(log_path: &Path, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let Ok(file) = File::open(log_path) else {
        return Vec::new();
    };
    let mut events = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.contains("  find ")
            || line.contains("  retrieve ")
            || line.contains("  find_related ")
            || line.contains("  list_corpora ")
        {
            events.push(line);
            if events.len() > limit {
                events.remove(0);
            }
        }
    }
    events
}

/// Reads aggregate token-savings statistics from the corpus log.
pub(crate) fn read_stats(log_path: &Path) -> StatsReport {
    let mut report = StatsReport::default();
    let Ok(file) = File::open(log_path) else {
        return report;
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        // Successful `find` events carry the token estimates; skip failed calls
        // so the audit's error lines never distort the savings aggregate.
        if !line.contains("  find ") || line.contains("status=error") {
            continue;
        }
        report.queries += 1;
        if let Some(actual) = extract_kv_u64(&line, "actual_tok=") {
            report.actual_tokens += actual;
        }
        if let Some(cold) = extract_kv_u64(&line, "cold_tok=") {
            report.cold_tokens += cold;
            report.has_cold = true;
        }
    }
    report
}

/// Returns the timestamp of the last `serve start` event in the log, if any.
pub(crate) fn last_serve_start(log_path: &Path) -> Option<String> {
    let file = File::open(log_path).ok()?;
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| line.contains("  serve start  "))
        .last()
        .and_then(|line| line.split("  serve start  ").next().map(str::to_owned))
}

// Internal helpers

fn open_log_file(path: &Path) -> Option<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// Estimates the "tokens if cold" for a set of find results: sum of live source
/// file sizes divided by 4. Returns `None` when the corpus root is unknown or
/// no source files could be measured.
fn cold_tokens(headers: &[SearchHeader], corpus_root: Option<&Path>) -> Option<u64> {
    let root = corpus_root?;
    if headers.is_empty() {
        return None;
    }
    let mut total_bytes: u64 = 0;
    let mut seen: HashSet<&str> = HashSet::new();
    for header in headers {
        if seen.insert(header.source_path.as_str()) {
            total_bytes += fs::metadata(root.join(&header.source_path))
                .map(|m| m.len())
                .unwrap_or(0);
        }
    }
    if total_bytes == 0 {
        None
    } else {
        Some(total_bytes / 4)
    }
}

fn extract_kv_u64(line: &str, key: &str) -> Option<u64> {
    let start = line.find(key)? + key.len();
    let end = line[start..]
        .find(|c: char| !c.is_ascii_digit())
        .map(|n| start + n)
        .unwrap_or(line.len());
    line[start..end].parse().ok()
}

// Timestamp (UTC, no external dependency)

fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_epoch_utc(secs)
}

pub(crate) fn format_epoch_utc(secs: u64) -> String {
    let time_of_day = secs % 86400;
    let days = secs / 86400;
    let hour = time_of_day / 3600;
    let min = (time_of_day % 3600) / 60;
    let sec = time_of_day % 60;

    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02} {hour:02}:{min:02}:{sec:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero_is_unix_epoch() {
        assert_eq!(format_epoch_utc(0), "1970-01-01 00:00:00");
    }

    #[test]
    fn known_date_formats_correctly() {
        // 2026-06-26 12:00:00 UTC  =  20630 days * 86400 + 43200  =  1_782_475_200
        assert_eq!(format_epoch_utc(1_782_475_200), "2026-06-26 12:00:00");
    }

    #[test]
    fn kv_extraction_parses_inline_numbers() {
        let line = "2026-06-26 09:14:09  find  q=\"x\"  results=3  actual_tok=1287  cold_tok=4040  ~3.1x saved";
        assert_eq!(extract_kv_u64(line, "actual_tok="), Some(1287));
        assert_eq!(extract_kv_u64(line, "cold_tok="), Some(4040));
        assert_eq!(extract_kv_u64(line, "results="), Some(3));
    }

    #[test]
    fn stats_reader_aggregates_find_lines() {
        use tempfile::NamedTempFile;
        let mut tmp = NamedTempFile::new().expect("tempfile");
        writeln!(
            tmp,
            "2026-06-26 09:14:02  serve start  model=test  embeddings=current"
        )
        .expect("write");
        writeln!(
            tmp,
            "2026-06-26 09:14:09  find         q=\"a\"  results=3  actual_tok=100  cold_tok=300"
        )
        .expect("write");
        writeln!(
            tmp,
            "2026-06-26 09:15:00  find         q=\"b\"  results=2  actual_tok=50  cold_tok=200"
        )
        .expect("write");
        writeln!(
            tmp,
            "2026-06-26 09:15:21  retrieve     chunk=abc  path=foo.rs"
        )
        .expect("write");
        tmp.flush().expect("flush");

        let stats = read_stats(tmp.path());
        assert_eq!(stats.queries, 2);
        assert_eq!(stats.actual_tokens, 150);
        assert_eq!(stats.cold_tokens, 500);
        assert!(stats.has_cold);
    }

    #[test]
    fn recent_events_returns_tail_of_find_and_retrieve_lines() {
        use tempfile::NamedTempFile;
        let mut tmp = NamedTempFile::new().expect("tempfile");
        writeln!(
            tmp,
            "2026-06-26 09:14:02  serve start  model=test  embeddings=current"
        )
        .expect("write");
        writeln!(
            tmp,
            "2026-06-26 09:14:09  find         q=\"a\"  results=3  actual_tok=100"
        )
        .expect("write");
        writeln!(
            tmp,
            "2026-06-26 09:15:21  retrieve     chunk=abc  path=foo.rs"
        )
        .expect("write");
        writeln!(
            tmp,
            "2026-06-26 09:16:09  find         q=\"b\"  results=1  actual_tok=50"
        )
        .expect("write");
        tmp.flush().expect("flush");

        let events = recent_events(tmp.path(), 2);
        assert_eq!(events.len(), 2);
        assert!(events[0].contains("  retrieve "));
        assert!(events[1].contains("q=\"b\""));
    }
}
