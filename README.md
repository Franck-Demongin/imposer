# Imposer

Application Rust d'imposition de documents (PDF, PNG, JPEG, PSD) sur des planches aux
formats A4, A3 et SRA3.

## Fonctionnalités

- Imposition automatique la plus dense, avec rotation individuelle optionnelle.
- Fond perdu ajustable, par défaut 2 mm.
- Marge de sécurité non imprimable fixe, par défaut 5 mm.
- Marques de coupe calculées à partir des bords de coupe uniques.
- Conservation du profil colorimétrique (`OutputIntent`) et des ressources des sources PDF.
- Recto/verso pour 1 ou 2 sources : pages d'un PDF ou plusieurs fichiers.
- Entrées raster supportées : PNG, JPEG/JPG et PSD. Le PSD CMJN doté d'un profil
  ICC intégré est converti en sRGB à partir de ce profil ; sinon le CMJN est
  préservé.
- Interface texte (TUI) accessible sans argument ni avec `--tui`.
- CLI.

## Formats d'entrée

- `PDF` : 1 ou 2 pages.
- `PNG` et `JPEG/JPG` : conversion en RVB. Le DPI est lu s'il est présent,
  sinon 300 DPI est utilisé.
- `PSD` : RGB, niveaux de gris et CMJN supportés.

Les dimensions d'entrée sont interprétées telles quelles, y compris le fond perdu déjà
présent dans le document source.

## Installation

### Prérequis

Installez Rust via [rustup](https://rustup.rs/) si nécessaire, puis vérifiez
l'installation :

```sh
rustc --version
cargo --version
```

### Récupérer les sources

```sh
git clone https://github.com/Franck-Demongin/imposer.git
cd imposer
```

### Compiler

```sh
cargo build --release
```

Le binaire est généré selon le système :

- Linux et macOS : `target/release/imposer`
- Windows : `target/release/imposer.exe`

### Lancer l'application

- Linux et macOS :
  ```sh
  ./target/release/imposer --tui
  ```
- Windows (PowerShell) :
  ```powershell
  .\target\release\imposer.exe --tui
  ```

Sans argument ni chemin de sortie, l'application ouvre aussi directement la TUI.

## Interface texte (TUI)

![Capture de l'interface TUI](assets/screen_tui.png)

```sh
./target/release/imposer --tui
```

Sans argument ni chemin de sortie, l'application ouvre aussi la TUI :

```sh
./target/release/imposer
```

La TUI permet de parcourir les fichiers `pdf`, `png`, `jpg`, `jpeg` et `psd`,
d'ajouter jusqu'à deux sources dans l'ordre de sélection avec le fichier par
défaut comme recto, de retirer la dernière source avec `Suppr`, et d'ajuster les
options avant l'exécution. Un seul PDF peut fournir ses deux pages ; avec
plusieurs sources, seule la première page de chaque PDF est utilisée.


## Interface graphique optionnelle (Slint)

La GUI est disponible uniquement en compilant avec la feature optionnelle
`slint-ui` ou `gui` (alias) :

```sh
cargo build --release --features gui
./target/release/imposer-gui
```
ou en développement :
```sh
cargo run --features gui --bin imposer-gui
```

Ce build produit deux exécutables : `imposer` (CLI et TUI) et `imposer-gui`
(interface graphique Slint), dont les deux sont publiés par la release.

## Icône et intégration au bureau

### Linux

La barre des tâches lit l'icône via le fichier `.desktop` et l'`app_id`
`io.github.Franck-Demongin.Imposer`. Pour installer localement :

```sh
cargo build --release --features gui --bin imposer-gui
./packaging/linux/install-local.sh
```

Le script copie le binaire `target/release/imposer-gui` et
`packaging/linux/io.github.Franck-Demongin.Imposer.desktop`, installe les PNG et
le SVG dans `~/.local/share/icons/hicolor`, puis met à jour les caches s'ils sont
disponibles. Pour supprimer cette installation :

```sh
./packaging/linux/uninstall-local.sh
```

Avec `cargo run`, GNOME n'affiche l'icône correcte qu'après installation du
`.desktop`. Le fichier est également fourni dans `packaging/linux`.

### Windows

L'icône est embarquée à la compilation dans l'exécutable `imposer-gui.exe` via
`winresource` et `build.rs` (`assets/icons/imposer.ico`). Aucune action manuelle
n'est nécessaire.

### Remplacer l'icône

Remplacez `assets/icons/imposer.svg` et les fichiers PNG/ICO, ou exécutez
`assets/icons/generate.sh` avec `rsvg-convert`/ImageMagick si le fichier source
a changé. Les fichiers générés sont versionnés pour que le build normal ne
dépende pas de ces outils.

## Utilisation CLI

Une source :

```sh
./target/release/imposer \
  --input document.pdf \
  --output planche.pdf \
  --sheet a4 \
  --bleed 2 \
  --sheet-orientation auto
```

Recto/verso à partir de deux fichiers :

```sh
./target/release/imposer \
  --input recto.pdf \
  --input verso.pdf \
  --output planche.pdf \
  --sheet sra3
```

Options principales : `--sheet` (`a4`, `a3`, `sra3`), `--sheet-orientation`
(`auto`, `portrait`, `landscape`), `--rotate-verso` (force a 180° rotation of
verso sheets, default false), `--bleed` (mm), `--safety-margin` (mm),
`--no-rotate-copies`, `--tui`.

Les applications CMYK conservent les profils ICC et les intentions de sortie ;
les applications RVB sont écrites en DeviceRGB. La rotation des versos n'est
appliquée que si `--rotate-verso` est demandé.

## Tests

```sh
cargo test
cargo clippy
```

## Publication Windows

La GitHub Action `.github/workflows/release.yml` compile et publie `imposer.exe`
lors d'un push de tag `v*`.
