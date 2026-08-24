use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Read-only Git observation used by a reader or an external workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub root: PathBuf,
    pub branch: String,
    pub head: String,
    pub base: String,
    pub status: String,
    pub diff: String,
}

/// Read-only content prepared for the TUI checkpoint view.
#[derive(Debug, Clone)]
pub struct Scope {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
    pub previews: HashMap<PathBuf, String>,
    pub diff_lines: HashMap<PathBuf, HashMap<u32, DiffKind>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Added,
    Deleted,
}

impl Report {
    #[allow(dead_code)]
    pub fn render(&self) -> String {
        format!(
            "Checkpoint (lecture seule)\n\nroot: {}\nbranch: {}\nhead: {}\nbase: {}\n\nstatus:\n{}\n\ndiff depuis la base:\n{}",
            self.root.display(),
            if self.branch.is_empty() {
                "(detached)"
            } else {
                &self.branch
            },
            self.head,
            self.base,
            if self.status.is_empty() {
                "(propre)"
            } else {
                &self.status
            },
            if self.diff.is_empty() {
                "(aucune différence suivie)"
            } else {
                &self.diff
            },
        )
    }
}

pub fn inspect(path: &Path, base_ref: &str) -> Result<Report> {
    let root = PathBuf::from(git_output(path, &["rev-parse", "--show-toplevel"])?);
    let branch = git_output(path, &["branch", "--show-current"])?;
    let head = git_output(path, &["rev-parse", "HEAD"])?;
    let base = git_output(path, &["rev-parse", base_ref])?;
    let status = git_output(path, &["status", "--porcelain=v1", "-unormal"])?;
    let diff = git_output(path, &["diff", "--no-ext-diff", base_ref, "--"])?;

    Ok(Report {
        root,
        branch,
        head,
        base,
        status,
        diff,
    })
}

/// Build the checkpoint scope consumed by the reader.
///
/// Tracked changes are collected relative to `base_ref`; untracked files are
/// added from porcelain status and rendered with Git's no-index diff.
pub fn prepare(path: &Path, base_ref: &str, target_ref: Option<&str>) -> Result<Scope> {
    let report = inspect(path, base_ref)?;
    let mut paths = changed_paths(&report.root, base_ref, target_ref)?;
    paths.sort();
    paths.dedup();

    let mut previews = HashMap::new();
    let mut diff_lines = HashMap::new();
    for file in &paths {
        let diff = diff_for_path(&report.root, base_ref, target_ref, file)?;
        let target_content = read_target_content(&report.root, target_ref, file)?;
        let display_lines = diff_display_lines(&diff, &target_content);
        let mut content = display_lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if target_content.ends_with('\n') {
            content.push('\n');
        }
        diff_lines.insert(
            file.clone(),
            display_lines
                .iter()
                .enumerate()
                .filter_map(|(line, entry)| {
                    entry.kind.map(|kind| {
                        (
                            crate::document::checkpoint_source_line(file, line as u32),
                            kind,
                        )
                    })
                })
                .collect(),
        );
        let preview = crate::document::checkpoint_source(file, &content);
        previews.insert(file.clone(), preview);
    }

    Ok(Scope {
        root: report.root,
        paths,
        previews,
        diff_lines,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayLine {
    text: String,
    kind: Option<DiffKind>,
}

fn read_target_content(root: &Path, target_ref: Option<&str>, path: &Path) -> Result<String> {
    match target_ref {
        Some(target) => {
            let relative = path.strip_prefix(root).unwrap_or(path);
            let object = format!("{target}:{}", relative.to_string_lossy());
            let output = Command::new("git")
                .args(["-C", &root.to_string_lossy(), "show", "--format="])
                .arg(object)
                .output()
                .with_context(|| format!("could not read {} at {target}", path.display()))?;
            if output.status.success() {
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
            } else {
                Ok(String::new())
            }
        }
        None => Ok(std::fs::read_to_string(path).unwrap_or_default()),
    }
}

fn diff_display_lines(diff: &str, target_content: &str) -> Vec<DisplayLine> {
    let target: Vec<_> = target_content.lines().map(str::to_string).collect();
    let mut output = Vec::new();
    let mut target_cursor = 0usize;

    for line in diff.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            let Some(new_range) = header.split_whitespace().find(|part| part.starts_with('+'))
            else {
                continue;
            };
            let mut values = new_range.trim_start_matches('+').split(',');
            let start = values
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(1);
            while target_cursor < start.saturating_sub(1) {
                if let Some(text) = target.get(target_cursor) {
                    output.push(DisplayLine {
                        text: text.clone(),
                        kind: None,
                    });
                }
                target_cursor += 1;
            }
            continue;
        }

        if line.starts_with("+++") || line.starts_with("---") || line.starts_with('\\') {
            continue;
        }

        match line.as_bytes().first().copied() {
            Some(b'-') => output.push(DisplayLine {
                text: line[1..].to_string(),
                kind: Some(DiffKind::Deleted),
            }),
            Some(b'+') => {
                output.push(DisplayLine {
                    text: line[1..].to_string(),
                    kind: Some(DiffKind::Added),
                });
                target_cursor += 1;
            }
            Some(b' ') => {
                if let Some(text) = target.get(target_cursor) {
                    output.push(DisplayLine {
                        text: text.clone(),
                        kind: None,
                    });
                }
                target_cursor += 1;
            }
            _ => {}
        }
    }

    while target_cursor < target.len() {
        output.push(DisplayLine {
            text: target[target_cursor].clone(),
            kind: None,
        });
        target_cursor += 1;
    }
    output
}

fn changed_paths(root: &Path, base_ref: &str, target_ref: Option<&str>) -> Result<Vec<PathBuf>> {
    let mut paths = names_from_nul(git_raw(
        root,
        &match target_ref {
            Some(target) => vec![
                "diff",
                "--no-ext-diff",
                "--name-only",
                "-z",
                base_ref,
                target,
                "--",
            ],
            None => vec!["diff", "--no-ext-diff", "--name-only", "-z", base_ref, "--"],
        },
    )?)
    .into_iter()
    .map(|path| root.join(path))
    .collect::<Vec<_>>();
    let status = if target_ref.is_none() {
        git_raw(root, &["status", "--porcelain=v1", "-z", "-uall"])?
    } else {
        Vec::new()
    };
    let records: Vec<_> = status
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
        .collect();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        if record.len() > 3 {
            let code = &record[..2];
            paths.push(root.join(String::from_utf8_lossy(&record[3..]).as_ref()));
            if (code.starts_with(b"R") || code.starts_with(b"C")) && index + 1 < records.len() {
                index += 1;
                paths.push(root.join(String::from_utf8_lossy(records[index]).as_ref()));
            }
        }
        index += 1;
    }
    Ok(paths)
}

fn names_from_nul(raw: Vec<u8>) -> Vec<PathBuf> {
    raw.split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
        .map(|item| PathBuf::from(String::from_utf8_lossy(item).as_ref()))
        .collect()
}

fn diff_for_path(
    root: &Path,
    base_ref: &str,
    target_ref: Option<&str>,
    path: &Path,
) -> Result<String> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut command = Command::new("git");
    command.args(["-C", &root.to_string_lossy(), "diff", "--no-ext-diff"]);
    command.arg(base_ref);
    if let Some(target) = target_ref {
        command.arg(target);
    }
    let output = command
        .arg("--")
        .arg(relative)
        .output()
        .with_context(|| format!("could not calculate diff for {}", path.display()))?;
    if !output.status.success() {
        bail!(
            "git diff failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let diff = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string();
    if target_ref.is_some() || !diff.is_empty() || !path.exists() {
        return Ok(diff);
    }

    // `git diff BASE` does not include untracked files. A no-index comparison
    // gives them the same readable patch representation as tracked files.
    let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let output = Command::new("git")
        .args(["diff", "--no-ext-diff", "--no-index", "--", empty])
        .arg(path)
        .output()
        .with_context(|| format!("could not calculate untracked diff for {}", path.display()))?;
    if !matches!(output.status.code(), Some(0) | Some(1)) {
        bail!(
            "git diff --no-index failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

fn git_raw(path: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(["-C", &path.to_string_lossy()])
        .args(args)
        .output()
        .with_context(|| format!("could not run git in {}", path.display()))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn git_output(path: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(["-C", &path.to_string_lossy()])
        .args(args)
        .output()
        .with_context(|| format!("could not run git in {}", path.display()))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git_ok(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(["-C", &root.to_string_lossy()])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git command failed: {args:?}");
    }

    #[test]
    fn renders_empty_sections_without_fake_success() {
        let report = Report {
            root: PathBuf::from("/repo"),
            branch: String::new(),
            head: "abc".into(),
            base: "abc".into(),
            status: String::new(),
            diff: String::new(),
        };
        let text = report.render();
        assert!(text.contains("branch: (detached)"));
        assert!(text.contains("status:\n(propre)"));
        assert!(text.contains("(aucune différence suivie)"));
    }

    #[test]
    fn prepares_full_file_and_changed_lines_for_checkpoint_view() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("doc.md");
        let other = temp.path().join("code.rs");
        fs::write(&file, "# Original\n").unwrap();
        fs::write(&other, "fn old() {}\n").unwrap();
        git_ok(temp.path(), &["init", "-q"]);
        git_ok(temp.path(), &["config", "user.name", "Test"]);
        git_ok(temp.path(), &["config", "user.email", "test@example.com"]);
        git_ok(temp.path(), &["add", "doc.md", "code.rs"]);
        git_ok(temp.path(), &["commit", "-qm", "initial"]);
        fs::write(&file, "# Updated\n\nContext\n").unwrap();
        fs::write(&other, "fn updated() {}\n").unwrap();

        let scope = prepare(temp.path(), "HEAD", None).unwrap();
        assert!(scope.paths.contains(&file.canonicalize().unwrap()));
        assert!(scope.paths.contains(&other.canonicalize().unwrap()));
        let preview = scope.previews.get(&file.canonicalize().unwrap()).unwrap();
        assert_eq!(preview, "# Original\n# Updated\n\nContext\n");
        assert_eq!(
            scope.diff_lines.get(&file.canonicalize().unwrap()),
            Some(&HashMap::from([
                (0, DiffKind::Deleted),
                (1, DiffKind::Added),
                (2, DiffKind::Added),
                (3, DiffKind::Added),
            ])),
        );
    }

    #[test]
    fn builds_commit_to_commit_checkpoint() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("doc.md");
        fs::write(&file, "before\n").unwrap();
        git_ok(temp.path(), &["init", "-q"]);
        git_ok(temp.path(), &["config", "user.name", "Test"]);
        git_ok(temp.path(), &["config", "user.email", "test@example.com"]);
        git_ok(temp.path(), &["add", "doc.md"]);
        git_ok(temp.path(), &["commit", "-qm", "before"]);
        fs::write(&file, "after\n").unwrap();
        git_ok(temp.path(), &["commit", "-qam", "after"]);

        let scope = prepare(temp.path(), "HEAD~1", Some("HEAD")).unwrap();
        let path = file.canonicalize().unwrap();
        assert_eq!(scope.paths, vec![path.clone()]);
        assert_eq!(
            scope.previews.get(&path),
            Some(&"before\nafter\n".to_string())
        );
        assert_eq!(
            scope.diff_lines.get(&path),
            Some(&HashMap::from([
                (0, DiffKind::Deleted),
                (1, DiffKind::Added),
            ])),
        );
    }
}
