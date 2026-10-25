# Imposer

Application Rust d'imposition de documents (PDF, PNG, JPEG, PSD) sur des planches aux
formats A4, A3 et SRA3.

## Fonctionnalités

- Imposition automatique la plus dense, avec rotation individuelle optionnelle.
- Fond perdu ajustable, par défaut 2 mm.
- Marge de sécurité non imprimable fixe, par défaut 5 mm.
- Marques de coupe calculées à partir des bords de coupe uniques.
- Conservation du profil colorimétrique (`OutputIntent`), des ressources des
  sources PDF et du profil ICC des sources CMJN.
- Recto/verso pour 1 ou 2 sources : pages d'un PDF ou plusieurs fichiers.
- Les sources d'une même imposition doivent avoir le même type de colorimétrie
  (`RVB` ou `CMJN`) et des dimensions compatibles, à 0.5 pt près.
- Entrées raster supportées : PNG, JPEG/JPG et PSD. Les PSD peuvent être RGB,
  niveaux de gris ou CMJN ; un PSD CMJN doit contenir un profil ICC intégré. Le
  décodeur Rust lit le composite et préserve le type de colorimétrie et le profil
  ICC. Les sources RVB restent en DeviceRGB.
- Profil couleur détecté affiché lors de la génération (CLI et TUI).
- Interface texte (TUI) accessible sans argument ni avec `--tui`.
- CLI.

## Formats d'entrée

- `PDF` : 1 ou 2 pages, avec conservation des ressources et, lorsque les sources
  sont CMYK, des intentions de sortie compatibles.
- `PNG` et `JPEG/JPG` : conversion en RVB ; les profils ICC sont ignorés
  (DeviceRGB). Le DPI est lu s'il est présent, sinon 300 DPI est utilisé.
- `PSD` : RGB, niveaux de gris ou CMJN supportés. Un PSD CMJN doit contenir un
  profil ICC intégré ; sinon la source est refusée. La résolution est lue si elle
  est intégrée, sinon 300 DPI est utilisée. Le composite et le profil ICC
  sont lus directement par le décodeur Rust, sans dépendance à ImageMagick.
- Les profils CMJN PDF et PSD sont inspectés récursivement, y compris les
  intentions de sortie ICC.

Toutes les données d'une même imposition doivent avoir les mêmes dimensions, à
0,5 pt près, ainsi que la même nature colorimétrique. Les dimensions d'entrée
sont interprétées telles quelles, y compris le fond perdu déjà présent dans le
document source.

## Installation

### Prérequis

Installez Rust via [rustup](https://rustup.rs/) si nécessaire, puis vérifiez
l'installation :

```sh
rustc --version
cargo --version
```

Pour la commande CLI/TUI qui écrit le PDF certifié, Ghostscript doit être
installé et disponible (commande `gs` sous Linux/macOS, `gswin64c.exe` sous
Windows).

Pour traiter les PSD, le décodeur Rust lit le composite directement ; aucune
commande `magick` n'est nécessaire. Ghostscript reste requis pour la
certification PDF/X-4.

Pour compiler avec la GUI Slint (`--features gui`), les bibliothèques
Slint/Wayland sont également nécessaires sous Linux.

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

#### Installer depuis une release Git

```sh
cargo install --git https://github.com/Franck-Demongin/imposer.git --tag v0.6.3 --features gui
```

Cette installation produit `imposer` et `imposer-gui`. Si `~/.cargo/bin` n'est pas dans
`PATH`, ajoutez-le puis rechargez la configuration du shell :

```sh
export PATH="$HOME/.cargo/bin:$PATH"
```

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

La liste des formats contient les tailles intégrées (`A4`, `A3`, `SRA3`),
verrouillées, puis l'entrée `Personnalisé`. Choisir cette entrée ouvre une
pop-up pour saisir la largeur et la hauteur en millimètres ; les flèches
`Tab`, `Left`, `Right`, `Home` et `End` et `Enter` permettent d'éditer et de
valider, tandis qu'`Esc` annule en restaurant les dimensions précédentes. Une
action discrète « Sauvegarder ce format » ouvre une pop-up demandant un nom, puis
enregistre le format dans les préférences utilisateur. Les formats enregistrés sont
rechargés au prochain lancement et apparaissent à côté des formats intégrés. Le
format actif est rappelé dans la ligne « Feuille - A4 (21.0 x 29.7 cm) » et dans
le récapitulatif. Les préréglés sont stockés en TOML, dans `presets.toml`, selon
le système :
- Linux : `${XDG_CONFIG_HOME:-$HOME/.config}/imposer/presets.toml`
- macOS : `$HOME/Library/Application Support/imposer/presets.toml`
- Windows : `%APPDATA%\imposer\presets.toml` (ou, à défaut, `%LOCALAPPDATA%\imposer\presets.toml`,
  puis `%USERPROFILE%\AppData\Roaming\imposer\presets.toml`)
Les noms vides, trop longs ou déjà utilisés (formats intégrés inclus) sont refusés.


## Interface graphique optionnelle (Slint)

![Capture de l'interface graphique Slint](assets/screen_gui.png)

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

Options principales : `--sheet` (`a4`, `a3`, `sra3`), `--sheet-width-mm` et
`--sheet-height-mm` (dimensions personnalisées en millimètres, utilisées
ensemble), `--sheet-orientation` (`auto`, `portrait`, `landscape`),
`--rotate-verso` (force a 180° rotation of verso sheets, default false),
`--bleed` (mm), `--safety-margin` (mm), `--no-rotate-copies`, `--tui`.

Les sources CMYK conservent leur profil ICC et, si tous les profils sont
compatibles, les métadonnées PDF/X-4. Les sources RVB sont écrites en
DeviceRGB. Aucune conversion implicite entre les deux familles de couleur n'est
effectuée. La rotation des versos n'est appliquée que si `--rotate-verso` est
demandé.

## Tests

```sh
cargo test
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
cargo build --release --features slint-ui
```

## Publication

La GitHub Action `.github/workflows/release.yml` déclenche une publication lors d'un
push de tag `v*`. Le premier job crée la release et ses notes à partir des commits
depuis le tag précédent, puis publie sous Windows les binaires `imposer.exe` et
`imposer-gui.exe`. Un second job Linux, qui dépend du premier, ajoute ensuite
`imposer` et `imposer-gui` à la même release.
