# Imposer

Rust CLI/library for imposing PDF, PNG, JPEG/JPG, and PSD documents on A4, A3, and SRA3 sheets (v0.3.1).

## Project Overview

`imposer` reads 1- or 2-page PDFs and raster images and generates an imposed sheet using a Rust
2024 codebase, `clap` for the CLI, `ratatui`/`crossterm` for the TUI, and `lopdf 0.45` for PDF
generation.

Supported input handling:
- PDF: 1 or 2 pages, with preserved resources and OutputIntent profiles
- PNG and JPEG/JPG: converted to RGB; DPI is read when present, otherwise 300 DPI is used
- PSD: RGB, grayscale, and CMYK (CMYK with an embedded ICC profile is converted to sRGB)
- All sources passed to one imposition must have matching dimensions within `0.5 pt`

Main modules:
- `src/main.rs`: CLI entry point and argument mapping, TUI launch
- `src/lib.rs`: public exports (`ImpositionConfig`, `impose`, layout helpers)
- `src/tui.rs`: interactive ratatui/crossterm interface
- `src/config.rs`: sheet/document orientations, bleed, safety margin, copy rotation
- `src/geometry.rs`: mm/pt conversions
- `src/layout.rs`: sheet selection, grid computation, centered placement origins, trim boxes
- `src/source.rs`: source parsing and dimension detection
- `src/pdf.rs`: PDF parsing, Form XObject creation, deep resource copying, crop marks, OutputIntent preservation, recto/verso generation
- `src/error.rs`: typed errors

## Building and Running

Build:
```sh
cargo build
cargo build --release
```

Run TUI:
```sh
cargo run -- --tui
cargo run
```

Run CLI:
```sh
cargo run -- --input document.pdf --output out.pdf --sheet sra3 --bleed 2 --safety-margin 5
```

Installed binary usage:
```sh
./target/release/imposer --input document.pdf --output out.pdf --sheet sra3
./target/release/imposer --tui
```

CLI options:
- `--input`, `-i`: source PDF/image (`pdf`, `png`, `jpg`, `jpeg`, `psd`)
- `--output`, `-o`: output PDF
- `--sheet`: `a4`, `a3`, `sra3` (default `a4`)
- `--sheet-orientation`: `auto`, `portrait`, `landscape` (default `auto`)
- `--bleed`: bleed in mm (default `2.0`)
- `--safety-margin`: safety margin in mm (default `5.0`)
- `--document-orientation`: `portrait`, `landscape` (default `portrait`)
- `--no-rotate-copies`: disable 90° copy rotation when it improves fit
- `--tui`: open the interactive TUI (also opens automatically when no input/output is given)

## Testing

```sh
cargo test
cargo clippy
cargo fmt --check
```

Integration tests are in `tests/integration_test.rs`. Test fixtures are in
`tests/fixtures/`.

## Architectural Notes

- Input may be a single 1- or 2-page PDF, two single-page sources for recto/verso, or raster images.
- A single selected PDF source may contribute 1 or 2 pages; when several sources are selected, the TUI keeps only the first page of each source.
- Source page dimensions already include bleed; layout uses them directly and derives trim boxes as source size minus `2 * bleed`.
- All supplied sources must have matching dimensions within `0.5 pt`.
- Bleed values cannot exceed half the source dimensions.
- Layout candidates are selected by maximum copy count, then minimum waste.
- The placement grid is centered independently on horizontal and vertical axes; horizontal/vertical gaps can differ.
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

## Interactive TUI

- File browser supports PDF, PNG, JPEG, and PSD sources, with up to 2 selections.
- Output is configured through separate directory and filename fields; `.pdf` is appended automatically.
- Options include sheet size, sheet/document orientations, bleed, safety margin, and copy rotation.
- Generation runs outside the UI loop and shows a braille spinner in the status area while busy; duplicate runs are ignored.
- A centered `Générer` button supports keyboard and mouse activation.

## Release Workflow

Tag pushes matching `v*` trigger `.github/workflows/release.yml`, which builds
`imposer.exe` on Windows, creates a release named `Imposer <tag>`, and uploads
the executable with commit-derived notes since the previous tag.

## Repository Notes

- `target/` and `Cargo.lock` are gitignored in this repository.
- User-facing UI text is French; Rust identifiers and module names are English.
- Commit messages are written in English and use Conventional Commits formatting
  (`feat:`, `fix:`, `ci:`, `chore:`, etc.) so release-note generation and
  project history stay consistent. Tag names use the `v<version>` form, such as
  `v0.3.0`.
```