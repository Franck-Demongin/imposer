#[cfg(all(feature = "slint-ui", target_os = "windows"))]
fn main() {
    // Embarquer l'icône Windows et les métadonnées du produit.
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icons/imposer.ico");
    res.set(
        "FileDescription",
        "Imposition de documents PDF, PNG, JPEG et PSD",
    );
    res.set("ProductName", "Imposer");
    res.set("OriginalFilename", "imposer-gui.exe");
    res.compile()
        .expect("échec de la compilation des ressources Windows");
    #[cfg(feature = "slint-ui")]
    slint_build::compile("ui/main.slint").expect("échec de la compilation Slint");
}

#[cfg(all(feature = "slint-ui", not(target_os = "windows")))]
fn main() {
    slint_build::compile("ui/main.slint").expect("échec de la compilation Slint");
}

#[cfg(not(feature = "slint-ui"))]
fn main() {
    // Rien à faire hors de la feature GUI.
}
