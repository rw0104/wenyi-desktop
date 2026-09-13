//! The shelf: which books the user has added.
//!
//! Stored separately from run history. History answers "what has been translated"; the shelf
//! answers "what is on my desk", and it must survive the history being trimmed, so the two
//! are kept apart rather than derived from one another.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const LIBRARY_FILE: &str = "library.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub input: String,
    pub added_at: String,
}

fn path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(LIBRARY_FILE)
}

pub fn load(config_dir: &Path) -> Vec<LibraryEntry> {
    fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(config_dir: &Path, entries: &[LibraryEntry]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    fs::write(path(config_dir), text).map_err(|e| e.to_string())
}

/// Add books to the shelf, most recently added first, without duplicates.
///
/// Re-adding an existing book moves it to the front rather than creating a second copy: the
/// shelf is a set, and a duplicate entry would present two identical covers that translate
/// into the same state directory.
pub fn add(
    config_dir: &Path,
    inputs: &[String],
    now: &str,
) -> Result<Vec<LibraryEntry>, String> {
    let mut entries = load(config_dir);
    for input in inputs {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }
        entries.retain(|entry| entry.input != trimmed);
        entries.insert(
            0,
            LibraryEntry {
                input: trimmed.to_string(),
                added_at: now.to_string(),
            },
        );
    }
    save(config_dir, &entries)?;
    Ok(entries)
}

/// Remove one book from the shelf. The file itself is never touched.
pub fn remove(config_dir: &Path, input: &str) -> Result<Vec<LibraryEntry>, String> {
    let mut entries = load(config_dir);
    entries.retain(|entry| entry.input != input);
    save(config_dir, &entries)?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wenyi-library-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn starts_empty_and_round_trips() {
        let dir = temp_dir("empty");
        assert!(load(&dir).is_empty());

        add(&dir, &["a.epub".into()], "1").unwrap();
        let entries = load(&dir);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].input, "a.epub");
    }

    #[test]
    fn newest_book_is_first() {
        let dir = temp_dir("order");
        add(&dir, &["a.epub".into()], "1").unwrap();
        add(&dir, &["b.epub".into()], "2").unwrap();
        let inputs: Vec<String> = load(&dir).into_iter().map(|e| e.input).collect();
        assert_eq!(inputs, vec!["b.epub", "a.epub"]);
    }

    /// Two identical covers would both resolve to the same state directory, which reads as a
    /// bug rather than as two books.
    #[test]
    fn re_adding_moves_to_front_without_duplicating() {
        let dir = temp_dir("dupe");
        add(&dir, &["a.epub".into()], "1").unwrap();
        add(&dir, &["b.epub".into()], "2").unwrap();
        add(&dir, &["a.epub".into()], "3").unwrap();

        let entries = load(&dir);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].input, "a.epub");
        assert_eq!(entries[0].added_at, "3");
    }

    #[test]
    fn several_books_can_be_added_at_once() {
        let dir = temp_dir("multi");
        add(
            &dir,
            &["a.epub".into(), "b.epub".into(), "c.epub".into()],
            "1",
        )
        .unwrap();
        assert_eq!(load(&dir).len(), 3);
    }

    #[test]
    fn blank_inputs_are_ignored() {
        let dir = temp_dir("blank");
        add(&dir, &["".into(), "   ".into()], "1").unwrap();
        assert!(load(&dir).is_empty());
    }

    #[test]
    fn removing_keeps_the_rest() {
        let dir = temp_dir("remove");
        add(&dir, &["a.epub".into(), "b.epub".into()], "1").unwrap();
        let left = remove(&dir, "a.epub").unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].input, "b.epub");

        // Removing something absent is a no-op, not an error.
        let same = remove(&dir, "missing.epub").unwrap();
        assert_eq!(same.len(), 1);
    }
}
