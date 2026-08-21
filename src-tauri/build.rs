fn main() {
    // The manifest asks Windows for per-monitor DPI awareness and a modern
    // common-controls version; without it the app renders blurry on scaled
    // displays, which is most Windows laptops.
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));
    let attrs = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attrs).expect("failed to run tauri build script");
}
