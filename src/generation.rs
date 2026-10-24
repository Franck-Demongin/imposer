//! Orchestration réutilisable de la génération imprimée.
//!
//! Ce module concentre tout ce que partagent la CLI, la TUI et la future GUI :
//! lecture des sources, validation des options, appel de la primitive
//! d'imposition, écriture du fichier de sortie et rapport de génération.

use crate::config::ImpositionConfig;
use crate::error::{ImposerError, Result};
use crate::pdf::impose_sources;
use crate::source::SourcePage;
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
    let pipeline = crate::pipeline::PrintPipeline::new().map_err(ImposerError::Io)?;
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
