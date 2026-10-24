#![forbid(unsafe_code)]

pub mod config;
pub mod error;
pub mod generation;
pub mod geometry;
pub mod icc;
pub mod import;
pub mod layout;
pub mod numfmt;
pub mod open;
pub mod output;
pub mod pdf;
pub mod pipeline;
pub mod preflight;
pub mod profile;
pub mod source;

pub use config::ImpositionConfig;
pub use config::ImpositionConfig as ImposeOptions;
pub use error::{ImposerError, Result};
pub use generation::GenerateReport;
pub use generation::{generate, parse_source_bytes, read_source_pages, validate_options};
pub use geometry::{Size, mm_to_pt, pt_to_mm};
pub use icc::{IccProfile, ProfileMatch};
pub use import::ImportedPage;
pub use layout::{Backing, LayoutPlan, Placement, Rot90, SheetSpec, compute_layout};
pub use output::{OutputBuilder, OutputOptions};
pub use pdf::impose;
pub use pdf::impose_sources;
pub use pdf::{
    count_crop_marks, crop_mark_count, crop_mark_edges, crop_marks, crop_marks_count,
    draw_crop_marks,
};
pub use preflight::{PreflightReport, preflight, preflight_output, preflight_source};
pub use profile::{ProfileDetection, detect_cmyk_profile};
pub use source::SourceColorKind;
pub use source::{SourceKind, SourcePage, SourcePdf, open_pdf_source, sources_from_bytes};
