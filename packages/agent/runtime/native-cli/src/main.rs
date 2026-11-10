//! Small platform executable for the owned Node runtime. No provider or tool loop lives here.
use std::{
    env, fs, io,
    path::PathBuf,
    process::{self, Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

const SOURCES: &[(&str, &str)] = &[
    ("cli.mjs", include_str!("../../src/cli.mjs")),
    ("agent.mjs", include_str!("../../src/agent.mjs")),
    ("context.mjs", include_str!("../../src/context.mjs")),
    ("learning.mjs", include_str!("../../src/learning.mjs")),
    ("config.mjs", include_str!("../../src/config.mjs")),
    ("tools.mjs", include_str!("../../src/tools.mjs")),
    ("provider.mjs", include_str!("../../src/provider.mjs")),
    ("mcp.mjs", include_str!("../../src/mcp.mjs")),
];
struct Runtime(PathBuf);
impl Drop for Runtime {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run() -> io::Result<i32> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let dir = env::temp_dir().join(format!("athenaeum-agent-{}-{nonce}", process::id()));
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    // create_dir refuses an existing path, including attacker-controlled symlinks.
    builder.create(&dir)?;
    let runtime = Runtime(dir);
    for (name, source) in SOURCES {
        fs::write(runtime.0.join(name), source)?;
    }
    let node = env::var_os("ATHENAEUM_NODE").unwrap_or_else(|| "node".into());
    let status = Command::new(node)
        .arg(runtime.0.join("cli.mjs"))
        .args(env::args_os().skip(1))
        .stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit())
        .status().map_err(|error| io::Error::new(error.kind(), format!("Cannot start Node 22.19+: {error}. Install the packaged tools or set ATHENAEUM_NODE.")))?;
    Ok(status.code().unwrap_or(1))
}
fn main() {
    match run() {
        Ok(code) => process::exit(code),
        Err(error) => {
            eprintln!("Athenaeum Agent: {error}");
            process::exit(1);
        }
    }
}
