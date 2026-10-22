#[cfg(feature = "slint-ui")]
fn main() {
    slint_build::compile("ui/main.slint").unwrap();
}

#[cfg(not(feature = "slint-ui"))]
fn main() {
    // Rien à faire hors de la feature GUI.
}
