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
pub fn prepare(path: &Path, base_ref: &str) -> Result<Scope> {
    let report = inspect(path, base_ref)?;
    let mut paths = changed_paths(&report.root, base_ref)?;
    paths.sort();
    paths.dedup();

    let mut previews = HashMap::new();
    for file in &paths {
        let diff = diff_for_path(&report.root, base_ref, file)?;
        let preview = match std::fs::read_to_string(file) {
            Ok(content) => crate::document::checkpoint_preview(file, &content, &diff, base_ref),
            Err(_) => crate::document::diff_preview(&diff),
        };
        previews.insert(file.clone(), preview);
    }

    Ok(Scope {
        root: report.root,
        paths,
        previews,
    })
}

fn changed_paths(root: &Path, base_ref: &str) -> Result<Vec<PathBuf>> {
    let mut paths = names_from_nul(git_raw(
        root,
        &["diff", "--no-ext-diff", "--name-only", "-z", base_ref, "--"],
    )?)
    .into_iter()
    .map(|path| root.join(path))
    .collect::<Vec<_>>();
    let status = git_raw(root, &["status", "--porcelain=v1", "-z", "-uall"])?;
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

fn diff_for_path(root: &Path, base_ref: &str, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let output = Command::new("git")
        .args([
            "-C",
            &root.to_string_lossy(),
            "diff",
            "--no-ext-diff",
            base_ref,
            "--",
        ])
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
    if !diff.is_empty() || !path.exists() {
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
    fn prepares_full_file_and_diff_for_checkpoint_view() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("doc.md");
        fs::write(&file, "# Original\n").unwrap();
        git_ok(temp.path(), &["init", "-q"]);
        git_ok(temp.path(), &["config", "user.name", "Test"]);
        git_ok(temp.path(), &["config", "user.email", "test@example.com"]);
        git_ok(temp.path(), &["add", "doc.md"]);
        git_ok(temp.path(), &["commit", "-qm", "initial"]);
        fs::write(&file, "# Updated\n\nContext\n").unwrap();

        let scope = prepare(temp.path(), "HEAD").unwrap();
        assert_eq!(scope.paths, vec![file.canonicalize().unwrap()]);
        let preview = scope.previews.get(&file.canonicalize().unwrap()).unwrap();
        assert!(preview.contains("# Updated"));
        assert!(preview.contains("Changes since `HEAD`"));
        assert!(preview.contains("+# Updated"));
    }
}
