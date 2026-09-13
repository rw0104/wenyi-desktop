//! Resume support: join the shell's run history with the engine's on-disk state.
//!
//! Identity comes from the engine's own `source_sha256`, so a run is matched to its state
//! directory by file *content*, not by name. That keeps resume correct even when a book is
//! renamed or two files share a title.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::settings::{self, HistoryEntry};

/// One resumable book, as presented to the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub input: String,
    pub input_exists: bool,
    pub command: String,
    pub updated_at: String,
    pub title: Option<String>,
    /// Whether the engine has state for this exact file content.
    pub has_state: bool,
    pub chapters_done: u32,
    pub chapters_total: u32,
    pub source_lang: Option<String>,
    pub target_lang: Option<String>,
    pub state_dir: Option<String>,
    /// Directory that receives exported books (beside the source file).
    pub output_dir: String,
    /// Output files already produced for this book.
    pub outputs: Vec<String>,
}

/// Stream a SHA-256 digest without loading the whole book into memory.
pub fn sha256_file(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buffer[..n]),
            Err(_) => return None,
        }
    }
    Some(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Digest cache keyed by path, invalidated by size and modification time.
///
/// Hashing a book is a full file read; the shelf refreshes progress while a translation runs,
/// and re-hashing a 20 MB EPUB on every refresh would be pure waste.
fn digest_cache() -> &'static Mutex<HashMap<String, (u64, u128, String)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (u64, u128, String)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// SHA-256 of a file, reusing a previous result while size and mtime are unchanged.
pub fn digest_of(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let key = path.to_string_lossy().to_string();
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    if let Ok(cache) = digest_cache().lock() {
        if let Some((size, stamp, digest)) = cache.get(&key) {
            if *size == meta.len() && *stamp == mtime {
                return Some(digest.clone());
            }
        }
    }
    let digest = sha256_file(path)?;
    if let Ok(mut cache) = digest_cache().lock() {
        cache.insert(key, (meta.len(), mtime, digest.clone()));
    }
    Some(digest)
}

/// Find the state directory whose manifest declares `digest` as its source identity.
fn find_state_by_digest(state_root: &Path, digest: &str) -> Option<(PathBuf, serde_json::Value)> {
    let books = fs::read_dir(state_root).ok()?;
    for book in books.flatten() {
        let targets = book.path().join("targets");
        let Ok(entries) = fs::read_dir(&targets) else {
            continue;
        };
        for target in entries.flatten() {
            let manifest_path = target.path().join("manifest.json");
            let Ok(text) = fs::read_to_string(&manifest_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            if manifest.get("source_sha256").and_then(|v| v.as_str()) == Some(digest) {
                return Some((target.path(), manifest));
            }
        }
    }
    None
}

fn count_chapters(manifest: &serde_json::Value) -> (u32, u32) {
    let chapters = manifest.get("chapters").and_then(|v| v.as_array());
    let Some(chapters) = chapters else {
        return (0, 0);
    };
    let total = chapters.len() as u32;
    let done = chapters
        .iter()
        .filter(|c| c.get("status").and_then(|s| s.as_str()) == Some("done"))
        .count() as u32;
    (done, total)
}

const OUTPUT_EXTENSIONS: [&str; 6] = ["epub", "docx", "txt", "html", "md", "pdf"];

/// List output files the engine has already produced beside `input`.
fn collect_outputs(input: &Path) -> Vec<String> {
    let Some(stem) = input.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let Some(parent) = input.parent() else {
        return Vec::new();
    };
    let dir = parent.join("output");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            let ext_ok = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| OUTPUT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
                .unwrap_or(false);
            name.starts_with(stem) && ext_ok
        })
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

fn summarize(state_root: &Path, entry: &HistoryEntry) -> RunSummary {
    let input_path = PathBuf::from(&entry.input);
    let input_exists = input_path.is_file();
    let output_dir = input_path
        .parent()
        .map(|p| p.join("output"))
        .unwrap_or_else(|| PathBuf::from("output"))
        .to_string_lossy()
        .into_owned();

    let mut summary = RunSummary {
        input: entry.input.clone(),
        input_exists,
        command: entry.command.clone(),
        updated_at: entry.updated_at.clone(),
        title: None,
        has_state: false,
        chapters_done: 0,
        chapters_total: 0,
        source_lang: None,
        target_lang: None,
        state_dir: None,
        output_dir,
        outputs: collect_outputs(&input_path),
    };

    if !input_exists {
        return summary;
    }
    let Some(digest) = digest_of(&input_path) else {
        return summary;
    };
    let Some((state_dir, manifest)) = find_state_by_digest(state_root, &digest) else {
        return summary;
    };

    let (done, total) = count_chapters(&manifest);
    summary.has_state = true;
    summary.chapters_done = done;
    summary.chapters_total = total;
    summary.title = manifest
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    summary.source_lang = manifest
        .get("source_lang")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    summary.target_lang = manifest
        .get("target_lang")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    summary.state_dir = Some(state_dir.to_string_lossy().into_owned());
    summary
}

/// Summarize a book by path, for callers that hold an input rather than a history record.
pub fn summarize_for(state_root: &Path, input: &str) -> RunSummary {
    summarize(
        state_root,
        &HistoryEntry {
            input: input.to_string(),
            command: String::new(),
            updated_at: String::new(),
        },
    )
}

/// Build the resume list from run history plus engine state.
pub fn list(workspace: &Path, config_dir: &Path) -> Vec<RunSummary> {
    let state_root = workspace.join("state");
    let history = settings::read_history(&config_dir.join(settings::HISTORY_FILE));
    history
        .iter()
        .map(|entry| summarize(&state_root, entry))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wenyi-runs-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sha256_matches_the_known_digest_of_empty_and_text_input() {
        let dir = temp_dir("sha");
        let file = dir.join("book.txt");
        fs::write(&file, b"abc").unwrap();
        // Well-known SHA-256 of "abc".
        assert_eq!(
            sha256_file(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        let empty = dir.join("empty.txt");
        fs::write(&empty, b"").unwrap();
        assert_eq!(
            sha256_file(&empty).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(sha256_file(&dir.join("missing.txt")), None);
    }

    #[test]
    fn chapter_counts_read_status_fields() {
        let manifest: serde_json::Value = serde_json::from_str(
            r#"{"chapters":[{"status":"done"},{"status":"pending"},{"status":"done"}]}"#,
        )
        .unwrap();
        assert_eq!(count_chapters(&manifest), (2, 3));

        let no_chapters: serde_json::Value = serde_json::from_str("{}").unwrap();
        assert_eq!(count_chapters(&no_chapters), (0, 0));
    }

    /// State is matched by content, so a renamed file still resumes.
    #[test]
    fn state_is_found_by_content_digest_not_by_name() {
        let root = temp_dir("digest");
        let state_root = root.join("state");
        let target = state_root.join("some-slug").join("targets").join("zh");
        fs::create_dir_all(&target).unwrap();

        let book = root.join("original.txt");
        fs::write(&book, b"chapter body").unwrap();
        let digest = sha256_file(&book).unwrap();

        fs::write(
            target.join("manifest.json"),
            format!(
                r#"{{"title":"Renamed Book","source_lang":"en","target_lang":"zh","source_sha256":"{digest}","chapters":[{{"status":"done"}},{{"status":"pending"}}]}}"#
            ),
        )
        .unwrap();

        let found = find_state_by_digest(&state_root, &digest).expect("digest must match");
        assert_eq!(found.1.get("title").unwrap(), "Renamed Book");

        let other = sha256_file(&{
            let p = root.join("other.txt");
            fs::write(&p, b"different body").unwrap();
            p
        })
        .unwrap();
        assert!(find_state_by_digest(&state_root, &other).is_none());
    }

    #[test]
    fn missing_input_is_reported_without_a_digest_lookup() {
        let root = temp_dir("missing");
        let state_root = root.join("state");
        fs::create_dir_all(&state_root).unwrap();

        let entry = HistoryEntry {
            input: root.join("gone.epub").to_string_lossy().into_owned(),
            command: "translate".into(),
            updated_at: "1".into(),
        };
        let summary = summarize(&state_root, &entry);
        assert!(!summary.input_exists);
        assert!(!summary.has_state);
        assert_eq!(summary.chapters_total, 0);
    }

    #[test]
    fn outputs_are_collected_beside_the_source() {
        let root = temp_dir("outputs");
        let out = root.join("output");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("book.zh.epub"), b"x").unwrap();
        fs::write(out.join("book.zh-bi.epub"), b"x").unwrap();
        fs::write(out.join("unrelated.epub"), b"x").unwrap();
        fs::write(out.join("book.notes.txt.keep"), b"x").unwrap();

        let found = collect_outputs(&root.join("book.epub"));
        assert_eq!(found.len(), 2, "got {found:?}");
        assert!(found.iter().all(|p| p.contains("book.zh")));
    }
}
