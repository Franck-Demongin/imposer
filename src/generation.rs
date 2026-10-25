//! Orchestration réutilisable de la génération imprimée.
//!
//! Ce module concentre tout ce que partagent la CLI, la TUI et la future GUI :
//! lecture des sources, validation des options, appel de la primitive
//! d'imposition, écriture du fichier de sortie et rapport de génération.

use crate::config::ImpositionConfig;
use crate::error::{ImposerError, Result};
use crate::geometry::mm_to_pt_f64;
use crate::icc::IccProfile;
use crate::import::{import_page, import_raster};
use crate::layout::compute_layout;
use crate::output::BoxSource;
use crate::output::{OutputBuilder, OutputOptions};
use crate::pdf::impose_sources;
use crate::pipeline::PrintPipeline;
use crate::source::{SourceColorKind, SourceKind, SourcePage};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

/// Rapport renvoyé après une génération réussie.
#[derive(Debug, Clone)]
pub struct GenerateReport {
    /// Emplacement effectivement écrit.
    pub output_path: PathBuf,
    /// Taille du fichier écrit, en octets.
    pub output_size: usize,
    /// Profils couleur détectés sur les sources, pour affichage UI.
    pub profiles: Vec<String>,
}

/// Lit les sources depuis des chemins et applique la règle multi-source
/// (une seule source peut contribuer toutes ses pages ; plusieurs sources ne
/// conservent que la première page de chaque fichier).
pub fn read_source_pages(paths: &[PathBuf]) -> Result<Vec<SourcePage>> {
    let mut input_bytes = Vec::with_capacity(paths.len());
    for path in paths {
        match fs::read(path) {
            Ok(b) => input_bytes.push(b),
            Err(e) => {
                return Err(ImposerError::Generation(format!(
                    "erreur lecture {} : {e}",
                    path.display()
                )));
            }
        }
    }

    let selected_count = input_bytes.len();
    let mut sources = Vec::new();
    for (offset, bytes) in input_bytes.into_iter().enumerate() {
        match crate::source::sources_from_bytes(std::slice::from_ref(&bytes)) {
            Ok(parsed) => {
                if selected_count > 1 && parsed.len() > 1 {
                    if let Some(first) = parsed.first() {
                        sources.push(first.clone());
                    } else {
                        return Err(ImposerError::Generation(format!(
                            "source vide pour \"{}\".",
                            paths[offset].display()
                        )));
                    }
                } else {
                    sources.extend(parsed);
                }
            }
            Err(e) => {
                return Err(ImposerError::Generation(format!(
                    "erreur source \"{}\" : {e}",
                    paths[offset].display()
                )));
            }
        }
    }
    Ok(sources)
}

/// Variante acceptant déjà des octets, utile pour les tests et les UI qui
/// conservent des données déjà chargées.
pub fn parse_source_bytes(inputs: &[Vec<u8>], force_first_page: bool) -> Result<Vec<SourcePage>> {
    let selected_count = inputs.len();
    let mut sources = Vec::new();
    for bytes in inputs {
        let parsed = crate::source::sources_from_bytes(std::slice::from_ref(bytes))?;
        if force_first_page && selected_count > 1 && parsed.len() > 1 {
            if let Some(first) = parsed.first() {
                sources.push(first.clone());
            } else {
                return Err(ImposerError::Generation("source vide".to_string()));
            }
        } else {
            sources.extend(parsed);
        }
    }
    Ok(sources)
}

/// Génère le PDF en imposant `sources` selon `options`, l'écrit à `output_path`
/// et renvoie un rapport contenant le chemin et la taille.
pub fn generate(
    sources: &[SourcePage],
    options: &ImpositionConfig,
    output_path: PathBuf,
) -> Result<GenerateReport> {
    options.validate()?;

    let profiles: Vec<String> = sources.iter().map(SourcePage::color_label).collect();

    if let Ok(report) = try_native_generate(sources, options, output_path.clone()) {
        return Ok(GenerateReport { profiles, ..report });
    }

    generate_with_ghostscript(sources, options, output_path, profiles)
}

fn try_native_generate(
    sources: &[SourcePage],
    options: &ImpositionConfig,
    output_path: PathBuf,
) -> Result<GenerateReport> {
    if sources.is_empty() {
        return Err(ImposerError::InvalidPageCount(0));
    }
    if sources.len() != 1 && sources.len() != 2 {
        return Err(ImposerError::InvalidPageCount(sources.len()));
    }

    if sources
        .iter()
        .any(|source| source.color_kind() != SourceColorKind::Cmyk)
    {
        return Err(ImposerError::Generation(
            "le moteur natif ne prend en charge que les sources CMJN".to_string(),
        ));
    }

    let first = &sources[0];
    let source_w = first.width_pt;
    let source_h = first.height_pt;
    for source in sources {
        if (source.width_pt - source_w).abs() > 0.5 || (source.height_pt - source_h).abs() > 0.5 {
            return Err(ImposerError::Dimensions(
                "sources de dimensions hétérogènes".to_string(),
            ));
        }
    }

    let profiles: Vec<IccProfile> = sources
        .iter()
        .map(SourcePage::cmyk_profile)
        .collect::<Option<Vec<_>>>()
        .ok_or(ImposerError::NoProfile)?;
    let profile_hashes: BTreeSet<[u8; 32]> = profiles.iter().map(|p| p.info.sha256).collect();
    if profile_hashes.len() > 1 {
        return Err(ImposerError::IncompatibleProfiles(
            sources
                .iter()
                .map(SourcePage::color_label)
                .collect::<Vec<_>>()
                .join(" ; "),
        ));
    }

    let source_bleed_pt = mm_to_pt_f64(options.bleed_mm as f64);
    let output_options = OutputOptions {
        source_bleed_pt,
        creation_date: "D:20260101000000Z".to_string(),
        mod_date: "D:20260101000000Z".to_string(),
        box_source: BoxSource::Bleed,
        ..OutputOptions::default()
    };

    let mut builder = OutputBuilder::new(profiles[0].clone(), output_options);

    for source in sources {
        // Avoid stale object-ID remapping across independent source documents
        // (the deep-copy cache key is only a numeric object id): clear it for
        // each source while still deduplicating within one import.
        builder.resource_cache.clear();
        let imported = match &source.kind {
            SourceKind::PdfPage { bytes, page_index } => {
                let src = crate::source::open_pdf_source(bytes.clone())?;
                import_page(&mut builder, &src, *page_index)?
            }
            SourceKind::Raster { .. } => import_raster(&mut builder, source)?,
        };
        builder.register_imported(imported);
    }

    let plan = compute_layout(source_w, source_h, options)?;
    let spec = crate::layout::SheetSpec {
        width_pt: plan.sheet_width as f64,
        height_pt: plan.sheet_height as f64,
        bleed_pt: source_bleed_pt,
        safety_margin_pt: crate::geometry::mm_to_pt_f64(5.0),
        trim_x_pt: 0.0,
        trim_y_pt: 0.0,
        trim_w_pt: plan.sheet_width as f64,
        trim_h_pt: plan.sheet_height as f64,
    };

    let placements = plan.placements.clone();
    for source_index in 0..sources.len() {
        let face_rotated = options.rotate_verso && source_index == 1;
        builder.add_sheet_full(
            &spec,
            &placements,
            source_index,
            plan.item_rotated,
            face_rotated,
        )?;
    }

    builder.write_to(&output_path)?;

    let output_size = fs::metadata(&output_path).map_err(ImposerError::Io)?.len() as usize;
    Ok(GenerateReport {
        output_path,
        output_size,
        profiles: Vec::new(),
    })
}

fn generate_with_ghostscript(
    sources: &[SourcePage],
    options: &ImpositionConfig,
    output_path: PathBuf,
    profiles: Vec<String>,
) -> Result<GenerateReport> {
    // 1. Générer d'abord le PDF hybride lopdf dans un fichier temporaire.
    let hybrid_bytes = impose_sources(sources, options)?;
    let hybrid_dir = std::env::temp_dir();
    let hybrid_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let hybrid_path = hybrid_dir.join(format!("imposer-hybrid-{}.pdf", hybrid_id));
    fs::write(&hybrid_path, &hybrid_bytes)?;

    // 2. Certifier le PDF hybride en PDF/X-4 avec Ghostscript.
    let pipeline = PrintPipeline::new().map_err(ImposerError::Io)?;
    pipeline.finalize_pdf_x(
        hybrid_path.to_string_lossy().as_ref(),
        output_path.to_string_lossy().as_ref(),
    )?;

    let _ = fs::remove_file(&hybrid_path);

    let output_size = fs::metadata(&output_path).map_err(ImposerError::Io)?.len() as usize;

    Ok(GenerateReport {
        output_path,
        output_size,
        profiles,
    })
}

/// Aide pour écrire `options.validate()` depuis l'UI sans connaître `config`.
pub fn validate_options(options: &ImpositionConfig) -> Result<()> {
    options.validate()
}
