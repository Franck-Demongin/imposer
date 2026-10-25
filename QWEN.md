# Imposer

Rust CLI/library for imposing PDF, PNG, JPEG/JPG, and PSD documents on A4, A3, and SRA3 sheets (v0.8.2).

## Project Overview

`imposer` reads 1- or 2-page PDFs and raster images and generates an imposed sheet using a Rust
2024 codebase, `clap` for the CLI, `ratatui`/`crossterm` for the TUI, and `lopdf 0.45` for PDF
generation.

Supported input handling:
- PDF: 1 or 2 pages, with preserved resources, OutputIntent profiles, and ICC-based CMYK profiles
- PNG and JPEG/JPG: converted to RGB; DPI is read when present, otherwise 300 DPI is used; ICC profiles are ignored (DeviceRGB)
- PSD: RGB, grayscale, and CMYK; CMYK requires an embedded ICC profile (CMYK without a profile is rejected); embedded PSD resolution is used when present, otherwise 300 DPI is assumed, and failed CMYK PSD decoding is not silently converted to RGB
- All sources in one imposition must have the same color kind (RGB or CMYK)
- All CMYK sources in one imposition must have compatible ICC profiles; PDF and PSD CMYK profile streams are compared by SHA-256, and distinct profiles are rejected
- All sources passed to one imposition must have matching dimensions within `0.5 pt`

Main modules:
- `src/main.rs`: CLI entry point, argument mapping, color/profile reporting, TUI launch
- `src/lib.rs`: public exports (config, errors, geometry, ICC, import, layout, output, PDF, preflight, profile, source APIs)
- `src/tui.rs`: interactive ratatui/crossterm interface, source profile reporting
- `src/config.rs`: sheet orientation, bleed, safety margin, copy/verso rotation
- `src/geometry.rs`: mm/pt conversions
- `src/icc.rs`: ICC header parsing, profile descriptions, SHA-256 comparison
- `src/import.rs`: imported page metadata and page box propagation helpers
- `src/layout.rs`: sheet selection, grid computation, centered placement origins, trim boxes
- `src/source.rs`: format detection, source parsing, dimensions, color kind, PSD CMYK/ICC handling, profile access, PSD resolution extraction
- `src/profile.rs`: recursive ICC-based and OutputIntent CMYK profile detection
- `src/pdf.rs`: PDF parsing, Form XObjects, deep resource copying, ICC raster images, crop marks, OutputIntents, X4 metadata, recto/verso generation
- `src/preflight.rs`: PDF and raster source checks, output PDF/X checks, color policy, page boxes, annotations
- `src/output.rs`: alternate output builder based on imported pages (not used by current `pdf.rs` pipeline)
- `src/numfmt.rs`: numeric formatting helpers for output metadata
- `src/error.rs`: typed errors (including mixed RGB/CMYK and incompatible ICC profile conflicts)
- `src/generation.rs`: shared generation orchestration (CLI/TUI), tries native CMYK PDF/X-4 output and keeps Ghostscript as fallback

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
- `--rotate-verso`: force a 180° rotation of verso sheets (default `false`)
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
- All supplied sources must have the same color kind: RGB sources stay DeviceRGB; CMYK sources preserve ICC profiles.
- Bleed values cannot exceed half the source dimensions.
- Layout candidates are selected by maximum copy count, then minimum waste.
- The placement grid is centered independently on horizontal and vertical axes; horizontal/vertical gaps can differ.
- Recto and verso faces use the same placements. Verso is transformed with a
  180° rotation only when `--rotate-verso` is used; otherwise it keeps the same
  sheet coordinate system. When the 180° rotation is applied, crop marks follow
  the same transform.
- Deep copying is used for page resources, hierarchical forms, ICC streams, and OutputIntents.
- PDF and PSD sources are inspected recursively for GTS_PDFX OutputIntents and ICCBased CMYK profiles; multiple distinct ICC profiles are rejected.
- CMYK jobs with compatible profiles can use native PDF/X-4 emission; Ghostscript remains the fallback for RGB, incompatible or missing profiles, and native-generation failures.
- PSD CMYK sources require a valid embedded ICC profile, are not silently converted to RGB, use embedded PSD resolution when present (otherwise 300 DPI), and still reject CMYK without a profile. Mixtures of RGB and CMYK sources are rejected.
- CMYK profiles from mixed PDF and PSD source sets are compared by SHA-256 and treated as compatible only when those hashes match.
- PDF/X-4 metadata (OutputIntent, ICC-based streams, XMP, Info, `/ID`, and Media/Bleed/Trim boxes) is emitted when every source is CMYK and either one source declares an OutputIntent or the sources share a compatible ICC profile.

## Crop Marks

Crop marks are derived from unique trim-box edges after optional 90° rotation:
- X = deduplicated trim `x` and `trim_x + trim_w`
- Y = deduplicated trim `y` and `trim_y + trim_h`
- tolerance: `0.01 mm`
- mark count: `2 * (|X| + |Y|)`
- marks are drawn only in the outer sheet margins
- mark geometry: 2 mm, offset at least bleed (default 3 mm), 0.25 pt,
  registration color (`K 1 1 1 1`)
- insufficient sheet margin returns `ImposerError::CropMarksMargin`

## Interactive TUI

- File browser supports PDF, PNG, JPEG, and PSD sources, with up to 2 selections.
- Output is configured through separate directory and filename fields; `.pdf` is appended automatically.
- The detected source color profile is shown when a source is selected and after generation.
- Options include sheet size, sheet/document orientations, bleed, safety margin, and copy rotation.
- Generation runs outside the UI loop and shows a braille spinner in the status area while busy; duplicate runs are ignored.
- A centered `Générer` button supports keyboard and mouse activation.

## Release Workflow

Tag pushes matching `v*` trigger `.github/workflows/release.yml`, which builds
Windows (`imposer.exe`) and Linux (`imposer`) binaries, creates a release named
`Imposer <tag>` with commit-derived notes since the previous tag, and uploads
both executables. The Windows build generates the release notes; the Linux build
runs afterwards and adds its executable to the same release.

## Repository Notes

- `target/` and `Cargo.lock` are gitignored in this repository.
- User-facing UI text is French; Rust identifiers and module names are English.
- Commit messages are written in English and use Conventional Commits formatting
  (`feat:`, `fix:`, `ci:`, `chore:`, etc.) so release-note generation and
  project history stay consistent. Tag names use the `v<version>` form, such as
  `v0.3.0`.
```