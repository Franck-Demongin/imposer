pub mod config;
pub mod error;
pub mod geometry;
pub mod layout;
pub mod pdf;
pub mod source;

pub use config::ImpositionConfig;
pub use error::{ImposerError, Result};
pub use layout::{LayoutPlan, Placement, compute_layout};
pub use pdf::impose;
pub use pdf::impose_sources;
pub use pdf::{
    count_crop_marks, crop_mark_count, crop_mark_edges, crop_marks, crop_marks_count,
    draw_crop_marks,
};
pub use source::{SourceKind, SourcePage, sources_from_bytes};
