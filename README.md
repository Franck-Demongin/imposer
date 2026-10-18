# Imposer

Application Rust d'imposition de documents PDF sur des planches aux formats A4, A3 et SRA3.

## Fonctionnalités

- Disposition automatique maximale (nesting) avec rotation individuelle optionnelle.
- Fond perdu ajustable (défaut 2 mm).
- Marge de sécurité non imprimable fixe (défaut 5 mm).
- Marques de coupe aux coins de chaque document.
- Conservation du profil colorimétrique (`OutputIntent`) et des ressources.
- Recto/verso automatique pour les PDF source de 1 ou 2 pages (verso tourné de 180°, centré).
- CLI.

## Compilation

```sh
cargo build --release
```

## Utilisation

```sh
./target/release/imposer \
  --input document.pdf \
  --output planche.pdf \
  --sheet a4 \
  --bleed 2 \
  --sheet-orientation auto
```

Options principales : `--sheet` (`a4`, `a3`, `sra3`), `--sheet-orientation` (`auto`, `portrait`, `landscape`), `--bleed` (mm), `--safety-margin` (mm), `--document-orientation` (`portrait`, `landscape`), `--no-rotate-copies`.

## Tests

```sh
cargo test
cargo clippy
```

## Publication Windows

La GitHub Action `.github/workflows/release.yml` compile et publie `imposer.exe` lors d'un push de tag `v*`.