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

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Athenaeum Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Athenaeum Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn reads_commit_metadata_and_changed_files() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "--quiet"]);
        std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
        git(dir.path(), &["add", "main.rs"]);
        git(
            dir.path(),
            &["commit", "--quiet", "-m", "initial structure"],
        );

        let (commits, warning) = read_history(dir.path(), 10);
        assert!(warning.is_none(), "{warning:?}");
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].subject, "initial structure");
        assert_eq!(commits[0].files, vec!["main.rs"]);
    }
}
