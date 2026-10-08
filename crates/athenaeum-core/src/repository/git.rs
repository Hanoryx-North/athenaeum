use std::path::Path;
use std::process::Command;

use super::GitCommit;

pub(super) fn read_history(root: &Path, max_commits: usize) -> (Vec<GitCommit>, Option<String>) {
    if max_commits == 0 || !root.join(".git").exists() {
        return (Vec::new(), None);
    }

    let output = match Command::new("git")
        .arg("--no-pager")
        .arg("-C")
        .arg(root)
        .arg("log")
        .arg(format!("--max-count={max_commits}"))
        .arg("--format=%x1e%H%x1f%aI%x1f%an%x1f%s")
        .arg("--name-only")
        .arg("--no-renames")
        .arg("-z")
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (
                Vec::new(),
                Some("Git history skipped because the git executable was not found".to_string()),
            );
        }
        Err(error) => {
            return (Vec::new(), Some(format!("Git history skipped: {error}")));
        }
    };

    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return (
            Vec::new(),
            Some(format!("Git history skipped: {}", detail.trim())),
        );
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut commits = Vec::new();
    for record in text
        .split('\u{1e}')
        .filter(|record| !record.trim().is_empty())
    {
        let mut parts = record.split('\0');
        let Some(header) = parts.next() else {
            continue;
        };
        let mut fields = header.trim().splitn(4, '\u{1f}');
        let (Some(id), Some(authored_at), Some(author), Some(subject)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let mut files = parts
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(normalize_path)
            .collect::<Vec<_>>();
        files.sort();
        files.dedup();
        commits.push(GitCommit {
            id: id.to_string(),
            author: author.to_string(),
            authored_at: authored_at.to_string(),
            subject: subject.to_string(),
            files,
        });
    }

    (commits, None)
}

fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}
