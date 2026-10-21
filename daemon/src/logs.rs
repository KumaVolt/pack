//! Log viewer: Laravel's storage/logs/*.log parsed into entries, plus Pack's
//! own process logs.

use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct Entry {
    pub time: String,
    pub env: String,
    pub level: String,
    pub message: String,
    pub context: String,
}

/// Log files of a project, newest first.
pub fn files(project: &Path) -> Vec<PathBuf> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(project.join("storage/logs"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "log"))
        .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), e.path()))
        .collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().map(|(_, p)| p).collect()
}

/// Read at most the last `max_bytes` of a file.
pub fn tail(path: &Path, max_bytes: u64) -> String {
    let Ok(mut file) = std::fs::File::open(path) else { return String::new() };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if len > max_bytes {
        let _ = file.seek(SeekFrom::Start(len - max_bytes));
    }
    let mut buf = vec![];
    let _ = file.read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Parse Monolog's default line format: `[2026-10-21 10:00:00] local.ERROR: message {context}`.
pub fn parse(text: &str, limit: usize) -> Vec<Entry> {
    let mut entries: Vec<Entry> = vec![];
    for line in text.lines() {
        if let Some(entry) = parse_head(line) {
            entries.push(entry);
        } else if let Some(last) = entries.last_mut() {
            last.context.push_str(line);
            last.context.push('\n');
        }
    }
    entries.reverse();
    entries.truncate(limit);
    entries
}

fn parse_head(line: &str) -> Option<Entry> {
    let rest = line.strip_prefix('[')?;
    let close = rest.find("] ")?;
    let time = &rest[..close];
    if !time.chars().next()?.is_ascii_digit() {
        return None;
    }
    let rest = &rest[close + 2..];
    let colon = rest.find(": ")?;
    let (env, level) = rest[..colon].split_once('.')?;
    if level.is_empty() || !level.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    Some(Entry {
        time: time.to_string(),
        env: env.to_string(),
        level: level.to_string(),
        message: rest[colon + 2..].to_string(),
        context: String::new(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_monolog_lines() {
        let text = "[2026-10-21 10:00:00] local.INFO: hello\n[2026-10-21 10:00:01] local.ERROR: boom {\"x\":1}\n#0 /app/foo.php(12)\n#1 {main}\n";
        let entries = super::parse(text, 10);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].level, "ERROR");
        assert!(entries[0].context.contains("#1 {main}"));
        assert_eq!(entries[1].message, "hello");
    }
}
