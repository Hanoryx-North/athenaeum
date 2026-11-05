fn main() -> std::io::Result<()> {
    println!("{}", codex_utils_home_dir::find_codex_home()?.display());
    Ok(())
}
