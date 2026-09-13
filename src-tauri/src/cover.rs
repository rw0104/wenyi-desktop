//! Cover art for the shelf.
//!
//! An EPUB is a zip and names its cover in the package document, so the cover is found by
//! declaration -- never by picking the largest image. Measured on a real magazine EPUB, the
//! two largest images were interior illustrations at 240 KB and 158 KB while the actual
//! cover was 146 KB, so the obvious heuristic chooses wrong.
//!
//! Formats without cover art (TXT, DOCX, PDF, SRT) return `None`; the shelf draws a
//! typographic placeholder instead of showing a hole.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A cover image plus the MIME type needed to embed it.
#[derive(Debug, Clone)]
pub struct Cover {
    pub bytes: Vec<u8>,
    pub mime: String,
}

fn mime_for(name: &str) -> String {
    match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => "image/jpeg",
    }
    .to_string()
}

/// Resolve a possibly relative href against the directory holding the OPF.
fn resolve(base_dir: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    if href.starts_with('/') {
        return href.trim_start_matches('/').to_string();
    }
    let mut parts: Vec<&str> = if base_dir.is_empty() {
        Vec::new()
    } else {
        base_dir.split('/').collect()
    };
    for segment in href.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Read one entry from an EPUB by full path, tolerating case differences in the archive.
fn read_entry(archive: &mut zip::ZipArchive<fs::File>, wanted: &str) -> Option<Vec<u8>> {
    let index = (0..archive.len()).find(|&i| {
        archive
            .by_index(i)
            .map(|entry| entry.name().eq_ignore_ascii_case(wanted))
            .unwrap_or(false)
    })?;
    let mut entry = archive.by_index(index).ok()?;
    let mut buffer = Vec::new();
    entry.read_to_end(&mut buffer).ok()?;
    Some(buffer)
}

fn entry_names(archive: &mut zip::ZipArchive<fs::File>) -> Vec<String> {
    (0..archive.len())
        .filter_map(|index| archive.by_index(index).ok().map(|e| e.name().to_string()))
        .collect()
}

/// Extract the text of an XML element's attribute without pulling in an XML parser.
///
/// The package document is machine-generated and the attributes searched for are stable, so
/// a narrow scan is proportionate here; it is deliberately not a general XML reader.
fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let needle = format!("{attr}=");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let body = &rest[1..];
    let end = body.find(quote)?;
    Some(body[..end].to_string())
}

/// Find the cover image path declared in an OPF document.
fn cover_href_from_opf(opf: &str) -> Option<String> {
    // EPUB 3: a manifest item flagged as the cover image.
    for tag in opf.split('<').filter(|t| t.starts_with("item ")) {
        if tag.contains("cover-image") {
            if let Some(href) = attr_value(tag, "href") {
                return Some(href);
            }
        }
    }
    // EPUB 2: a metadata pointer to a manifest item id.
    let mut cover_id = None;
    for tag in opf.split('<') {
        if tag.starts_with("meta ") && tag.contains("name=\"cover\"") {
            cover_id = attr_value(tag, "content");
            break;
        }
    }
    if let Some(id) = cover_id {
        for tag in opf.split('<').filter(|t| t.starts_with("item ")) {
            if attr_value(tag, "id").as_deref() == Some(id.as_str()) {
                return attr_value(tag, "href");
            }
        }
    }
    None
}

/// Extract the cover from an EPUB, or `None` when it has none.
pub fn epub_cover(path: &Path) -> Option<Cover> {
    let file = fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let names = entry_names(&mut archive);

    // Locate the package document through META-INF/container.xml, falling back to any .opf.
    let container = names
        .iter()
        .find(|n| n.eq_ignore_ascii_case("META-INF/container.xml"))
        .cloned();
    let opf_path = container
        .and_then(|name| read_entry(&mut archive, &name))
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|xml| {
            xml.split('<')
                .find(|t| t.starts_with("rootfile "))
                .and_then(|tag| attr_value(tag, "full-path"))
        })
        .or_else(|| names.iter().find(|n| n.ends_with(".opf")).cloned())?;

    let opf = String::from_utf8(read_entry(&mut archive, &opf_path)?).ok()?;
    let base_dir = opf_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("");
    let href = cover_href_from_opf(&opf)?;

    // The href is relative to the OPF; if that misses, try the name as-is.
    let candidates = [resolve(base_dir, &href), href.clone()];
    for candidate in candidates {
        if let Some(bytes) = read_entry(&mut archive, &candidate) {
            if !bytes.is_empty() {
                return Some(Cover {
                    mime: mime_for(&candidate),
                    bytes,
                });
            }
        }
    }
    None
}

/// Directory holding cached covers, created on demand.
pub fn cover_cache_dir(config_dir: &Path) -> Result<PathBuf, String> {
    let dir = config_dir.join("covers");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_hrefs_against_the_opf_directory() {
        assert_eq!(resolve("EPUB", "static_images/cover.jpg"), "EPUB/static_images/cover.jpg");
        assert_eq!(resolve("OEBPS/text", "../images/c.png"), "OEBPS/images/c.png");
        assert_eq!(resolve("", "cover.jpg"), "cover.jpg");
        assert_eq!(resolve("EPUB", "/abs/cover.jpg"), "abs/cover.jpg");
    }

    #[test]
    fn reads_the_epub3_cover_property() {
        let opf = r#"<package><manifest>
            <item id="i1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>
            <item id="cover-img" href="static_images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>
        </manifest></package>"#;
        assert_eq!(cover_href_from_opf(opf).as_deref(), Some("static_images/cover.jpg"));
    }

    #[test]
    fn reads_the_epub2_meta_pointer() {
        let opf = r#"<package><metadata><meta name="cover" content="cover-img"/></metadata>
            <manifest><item id="cover-img" href="images/c.png" media-type="image/png"/></manifest></package>"#;
        assert_eq!(cover_href_from_opf(opf).as_deref(), Some("images/c.png"));
    }

    /// The reason this module exists: the biggest image is usually an illustration.
    #[test]
    fn prefers_the_declared_cover_over_the_largest_image() {
        let opf = r#"<package><metadata><meta name="cover" content="cv"/></metadata><manifest>
            <item id="art1" href="big1.png" media-type="image/png"/>
            <item id="art2" href="big2.png" media-type="image/png"/>
            <item id="cv" href="cover.jpg" media-type="image/jpeg"/>
        </manifest></package>"#;
        assert_eq!(cover_href_from_opf(opf).as_deref(), Some("cover.jpg"));
    }

    #[test]
    fn a_package_without_a_cover_yields_nothing() {
        let opf = r#"<package><manifest>
            <item id="i1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
        </manifest></package>"#;
        assert_eq!(cover_href_from_opf(opf), None);
    }

    #[test]
    fn mime_is_derived_from_the_extension() {
        assert_eq!(mime_for("a/b/cover.PNG"), "image/png");
        assert_eq!(mime_for("cover.jpg"), "image/jpeg");
        assert_eq!(mime_for("cover.jpeg"), "image/jpeg");
        assert_eq!(mime_for("noext"), "image/jpeg");
    }
}
