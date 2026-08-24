use anyhow::{Context, Result, bail};
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

impl Report {
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
}
