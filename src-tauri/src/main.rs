// The desktop is a GUI even in an incremental standalone build.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    athenaeum_app_lib::run();
}
