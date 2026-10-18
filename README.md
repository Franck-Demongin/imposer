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

## Compilation

```sh
cargo build --release
```

## Interface texte (TUI)

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
(`auto`, `portrait`, `landscape`), `--bleed` (mm), `--safety-margin` (mm),
`--document-orientation` (`portrait`, `landscape`), `--no-rotate-copies`, `--tui`.

En mode `portrait` du document, le verso et ses marques de coupe sont tournés de
180°. En mode `landscape`, le système de coordonnées de la planche est conservé.

## Tests

```sh
cargo test
cargo clippy
```

## Publication Windows

La GitHub Action `.github/workflows/release.yml` compile et publie `imposer.exe`
lors d'un push de tag `v*`.
