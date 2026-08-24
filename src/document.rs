use anyhow::{Context, Result};
use ignore::{WalkBuilder, WalkState};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Text formats that the reader can show in its existing Markdown/code renderer.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "md", "markdown", "mmd", "json", "jsonl", "yaml", "yml", "toml", "txt",
];

/// Count every file extension under `root`, including formats this reader does
/// not display yet. This is intentionally separate from supported discovery.
pub fn format_inventory(root: &Path) -> BTreeMap<String, usize> {
    let inventory = Mutex::new(BTreeMap::new());
    WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(skip_generated_dirs)
        .build_parallel()
        .run(|| {
            let inventory = &inventory;
            Box::new(move |result| {
                let Ok(entry) = result else {
                    return WalkState::Continue;
                };
                if entry.file_type().is_some_and(|kind| kind.is_file()) {
                    let extension = entry
                        .path()
                        .extension()
                        .and_then(|value| value.to_str())
                        .map_or_else(|| "<none>".to_string(), |value| value.to_ascii_lowercase());
                    *inventory.lock().unwrap().entry(extension).or_default() += 1;
                }
                WalkState::Continue
            })
        });
    inventory.into_inner().unwrap_or_default()
}

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SUPPORTED_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
}

pub fn is_editable(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
        })
}

/// Return the smallest filesystem directory containing every authorized file.
pub fn common_root(paths: &[PathBuf]) -> Option<PathBuf> {
    let first = paths.first()?.parent()?.to_path_buf();
    let mut root = first;
    for path in paths.iter().skip(1) {
        while !path.starts_with(&root) {
            if !root.pop() {
                return None;
            }
        }
    }
    Some(root)
}

/// Read a newline-delimited manifest relative to `root` and expand directories
/// into authorized files. Blank lines and lines beginning with `#` are ignored.
pub fn read_manifest(root: &Path, manifest: &Path) -> Result<Vec<PathBuf>> {
    let text = std::fs::read_to_string(manifest)
        .with_context(|| format!("failed to read manifest {}", manifest.display()))?;
    let mut paths = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let candidate = manifest_path(line);
        let candidate = if candidate.is_absolute() {
            candidate
        } else {
            root.join(candidate)
        };
        let candidate = candidate
            .canonicalize()
            .with_context(|| format!("manifest path does not exist: {line}"))?;
        if candidate.is_dir() {
            paths.extend(walk_manifest_files(&candidate));
        } else {
            paths.push(candidate);
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn manifest_path(line: &str) -> PathBuf {
    #[cfg(unix)]
    if line.len() >= 3 && line.as_bytes()[1] == b':' {
        let drive = line[..1].to_ascii_lowercase();
        let rest = line[3..].replace('\\', "/");
        return PathBuf::from(format!("/mnt/{drive}/{rest}"));
    }
    PathBuf::from(line)
}

fn walk_manifest_files(root: &Path) -> Vec<PathBuf> {
    let paths = Mutex::new(Vec::new());
    WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(skip_generated_dirs)
        .build_parallel()
        .run(|| {
            Box::new(|result| {
                let Ok(entry) = result else {
                    return WalkState::Continue;
                };
                if entry.file_type().is_some_and(|kind| kind.is_file()) {
                    paths.lock().unwrap().push(entry.path().to_path_buf());
                }
                WalkState::Continue
            })
        });
    paths.into_inner().unwrap_or_default()
}

fn skip_generated_dirs(entry: &ignore::DirEntry) -> bool {
    !matches!(
        entry.file_name().to_str(),
        Some(".git") | Some(".graphify") | Some("node_modules") | Some("target")
    )
}

/// Keep Markdown as-is and turn structured documents into a readable preview.
/// JSON gets stable indentation; the other formats retain their source and use
/// the renderer's existing fenced-code highlighting.
pub fn display_source(path: &Path, content: &str) -> String {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);

    match extension.as_deref() {
        Some("md") | Some("markdown") => content.to_string(),
        Some("mmd") => fenced("mermaid", content),
        Some("json") => serde_json::from_str::<serde_json::Value>(content)
            .ok()
            .and_then(|value| serde_json::to_string_pretty(&value).ok())
            .map_or_else(|| fenced("json", content), |pretty| fenced("json", &pretty)),
        Some("jsonl") => {
            let pretty = content
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| {
                    serde_json::from_str::<serde_json::Value>(line)
                        .ok()
                        .and_then(|value| serde_json::to_string_pretty(&value).ok())
                        .unwrap_or_else(|| line.to_string())
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            fenced("json", if pretty.is_empty() { content } else { &pretty })
        }
        Some("yaml") | Some("yml") => fenced("yaml", content),
        Some("toml") => fenced("toml", content),
        Some("txt") => fenced("text", content),
        _ => content.to_string(),
    }
}

/// Render the current file for a read-only checkpoint view.
pub fn checkpoint_source(path: &Path, content: &str) -> String {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("md") | Some("markdown") => content.to_string(),
        Some("mmd") => fenced("mermaid", content),
        Some("json") | Some("jsonl") => fenced("json", content),
        Some("yaml") | Some("yml") => fenced("yaml", content),
        Some("toml") => fenced("toml", content),
        Some("txt") => fenced("text", content),
        Some(language) => fenced(language, content),
        None => fenced("text", content),
    }
}

/// Map a raw Git source line to its checkpoint display line.
pub fn checkpoint_source_line(path: &Path, source_line: u32) -> u32 {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    if matches!(extension.as_deref(), Some("md") | Some("markdown")) {
        source_line
    } else {
        source_line + 1
    }
}

fn fenced(language: &str, content: &str) -> String {
    format!("```{language}\n{}\n```\n", content.trim_end_matches('\n'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn recognizes_artifact_formats_case_insensitively() {
        assert!(is_supported(Path::new("decision.YAML")));
        assert!(is_supported(Path::new("graph.mmd")));
        assert!(!is_supported(Path::new("binary.png")));
    }

    #[test]
    fn preserves_markdown_source() {
        assert_eq!(
            display_source(Path::new("readme.md"), "# Title\n"),
            "# Title\n"
        );
    }

    #[test]
    fn formats_json_and_jsonl_for_preview() {
        assert_eq!(
            display_source(Path::new("project.json"), r#"{"name":"ia"}"#),
            "```json\n{\n  \"name\": \"ia\"\n}\n```\n"
        );
        assert!(
            display_source(Path::new("events.jsonl"), r#"{"ok":true}"#).contains("\"ok\": true")
        );
    }

    #[test]
    fn checkpoint_source_keeps_markdown_preview_and_fences_code() {
        assert_eq!(
            checkpoint_source(Path::new("readme.md"), "# Title"),
            "# Title"
        );
        assert_eq!(
            checkpoint_source(Path::new("main.rs"), "fn main() {}"),
            "```rs\nfn main() {}\n```\n"
        );
    }

    #[test]
    fn manifest_expands_directories_and_excludes_unsupported_files() {
        let temp = tempfile::tempdir().unwrap();
        let artifacts = temp.path().join("artifacts");
        std::fs::create_dir(&artifacts).unwrap();
        std::fs::write(artifacts.join("decision.yaml"), "status: expected\n").unwrap();
        std::fs::write(artifacts.join("notes.md"), "# Notes\n").unwrap();
        std::fs::write(artifacts.join("image.png"), "authorized text").unwrap();
        let manifest = temp.path().join("manifest.txt");
        std::fs::write(&manifest, "# active journey\nartifacts\n").unwrap();

        let paths = read_manifest(temp.path(), &manifest).unwrap();
        assert_eq!(paths.len(), 3);
        assert!(paths.iter().any(|path| path.ends_with("image.png")));
        assert_eq!(format_inventory(temp.path()).get("png"), Some(&1));
    }

    #[test]
    fn common_root_covers_files_from_multiple_project_locations() {
        let paths = vec![
            PathBuf::from("/library/docs/PUBLIC.md"),
            PathBuf::from("/library/lamoms/project.json"),
        ];
        assert_eq!(common_root(&paths), Some(PathBuf::from("/library")));
    }
}
