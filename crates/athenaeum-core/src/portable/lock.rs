//! A cross-process lock for the operations that write the vault.
//!
//! SQLite already serialises writers to `knowledge.db`. What it cannot protect is a *pair* of
//! writes — a page and the node that backs it, or a whole pull — so those hold this lock too. It is
//! a file created exclusively, which every platform the app ships on supports, and a lock older
//! than [`STALE_AFTER`] is taken over because its owner can only have crashed.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::portable_error;
use crate::error::Result;

const STALE_AFTER: Duration = Duration::from_secs(120);
const WAIT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub struct RepoLock {
    path: PathBuf,
}

impl RepoLock {
    /// Take the lock named `name` inside the workspace directory, waiting briefly for a holder.
    pub fn acquire(workspace: &Path, name: &str) -> Result<Self> {
        std::fs::create_dir_all(workspace)?;
        let path = workspace.join(format!("{name}.lock"));
        let started = std::time::Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    use std::io::Write;
                    let _ = writeln!(file, "{}", std::process::id());
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if is_stale(&path) {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    if started.elapsed() > WAIT {
                        return Err(portable_error(
                            "busy",
                            format!(
                                "another Athenaeum process is writing this repository ({}); \
                                 try again in a moment",
                                path.display()
                            ),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

impl Drop for RepoLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn is_stale(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age > STALE_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_is_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        {
            let _held = RepoLock::acquire(dir.path(), "vault").unwrap();
            assert!(dir.path().join("vault.lock").exists());
        }
        assert!(!dir.path().join("vault.lock").exists());
        RepoLock::acquire(dir.path(), "vault").unwrap();
    }
}
