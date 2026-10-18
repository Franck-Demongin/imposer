# Imposer

Rust CLI/library for imposing PDF documents on A4, A3, and SRA3 sheets.

## Project Overview

`imposer` reads a 1- or 2-page PDF and generates an imposed sheet using a Rust
2024 codebase, `clap` for the CLI, and `lopdf 0.45` for PDF generation.

Main modules:
- `src/main.rs`: CLI entry point and argument mapping
- `src/lib.rs`: public exports (`ImpositionConfig`, `impose`, layout helpers)
- `src/config.rs`: sheet/document orientations, bleed, safety margin, copy rotation
- `src/geometry.rs`: mm/pt conversions
- `src/layout.rs`: sheet selection, grid computation, centered placement origins, trim boxes
- `src/pdf.rs`: PDF parsing, Form XObject creation, deep resource copying, crop marks, OutputIntent preservation, recto/verso generation
- `src/error.rs`: typed errors

## Building and Running

Build:
```sh
cargo build
cargo build --release
```

Run:
```sh
cargo run -- --input document.pdf --output out.pdf --sheet sra3 --bleed 2 --safety-margin 5
```

Installed binary usage:
```sh
./target/release/imposer --input document.pdf --output out.pdf --sheet sra3
```

CLI options:
- `--input`, `-i`: source PDF
- `--output`, `-o`: output PDF
- `--sheet`: `a4`, `a3`, `sra3` (default `a4`)
- `--sheet-orientation`: `auto`, `portrait`, `landscape` (default `auto`)
- `--bleed`: bleed in mm (default `2.0`)
- `--safety-margin`: safety margin in mm (default `5.0`)
- `--document-orientation`: `portrait`, `landscape` (default `portrait`)
- `--no-rotate-copies`: disable 90° copy rotation when it improves fit

## Testing

```sh
cargo test
cargo clippy
cargo fmt --check
```

Integration tests are in `tests/integration_test.rs`. Test fixtures are in
`tests/fixtures/`.

## Architectural Notes

- Input PDF must contain exactly 1 or 2 pages.
- Source page dimensions already include bleed; layout uses them directly and
  derives trim boxes as source size minus `2 * bleed`.
- Bleed values cannot exceed half the source dimensions.
- Layout candidates are selected by maximum copy count, then minimum waste.
- The placement grid is centered independently on horizontal and vertical
  axes; horizontal/vertical gaps can differ.
- Recto and verso faces use the same placements. Verso is transformed with a
  180° rotation only when `--document-orientation portrait` is selected;
  otherwise it keeps the same sheet coordinate system. When the 180°
  rotation is applied, crop marks follow the same transform.
- Deep copying is used for page resources and OutputIntents.

## Crop Marks

Crop marks are derived from unique trim-box edges after optional 90° rotation:
- X = deduplicated trim `x` and `trim_x + trim_w`
- Y = deduplicated trim `y` and `trim_y + trim_h`
- tolerance: `0.01 mm`
- mark count: `2 * (|X| + |Y|)`
- marks are drawn only in the outer sheet margins
- mark geometry: 3 mm, offset at least bleed (default 3 mm), 0.25 pt,
  registration color (`K 1 1 1 1`)
- insufficient sheet margin returns `ImposerError::CropMarksMargin`

## Release Workflow

Tag pushes matching `v*` trigger `.github/workflows/release.yml`, which builds
`imposer.exe` on Windows and uploads it to the GitHub release.

## Repository Notes

- `target/` and `Cargo.lock` are gitignored in this repository.
- User-facing UI text is French; Rust identifiers and module names are English.
```