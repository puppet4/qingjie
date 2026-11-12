#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod geometry;
#[cfg(windows)]
mod win;
fn main() {
    #[cfg(windows)]
    win::run();
    #[cfg(not(windows))]
    eprintln!("轻截目前支持 Windows 10/11。");
}
