#![cfg_attr(all(windows, feature = "slint-ui"), windows_subsystem = "windows")]

#[cfg(feature = "slint-ui")]
#[allow(dead_code)]
mod tui;

#[cfg(feature = "slint-ui")]
mod slint_ui;

#[cfg(feature = "slint-ui")]
fn main() {
    if let Err(e) = slint_ui::run() {
        eprintln!("erreur Slint : {e}");
        std::process::exit(1);
    }
}
