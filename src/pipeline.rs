//! Pipeline optionnel certifié PDF/X-4 via Ghostscript/ImageMagick.
//!
//! Limite connue : Ghostscript et ImageMagick sont des dépendances système.
//! Le binaire Rust n'embarque que le fichier de configuration PostScript et
//! le profil ICC temporairement extraits.

use crate::error::{ImposerError, Result};
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

pub struct PrintPipeline {
    temp_dir: PathBuf,
    ps_config_path: PathBuf,
}

impl PrintPipeline {
    pub fn new() -> std::io::Result<Self> {
        let temp_dir = env::temp_dir();
        let ps_config_path = temp_dir.join("PDFX_def.ps");
        let icc_profile_path = temp_dir.join("ISOcoated_v2_300_eci.icc");

        let pdfx_def_bytes = include_bytes!("../assets/PDFX_def.ps");
        let icc_profile_bytes = include_bytes!("../assets/ISOcoated_v2_300_eci.icc");

        File::create(&ps_config_path)?.write_all(pdfx_def_bytes)?;
        File::create(&icc_profile_path)?.write_all(icc_profile_bytes)?;

        Ok(Self {
            temp_dir,
            ps_config_path,
        })
    }

    pub fn finalize_pdf_x(&self, input_pdf: &str, output_pdf: &str) -> Result<()> {
        let gs_cmd = if cfg!(target_os = "windows") {
            "gswin64c.exe"
        } else {
            "gs"
        };

        let output = Command::new(gs_cmd)
            .current_dir(&self.temp_dir)
            .args([
                "-dNOSAFER".to_string(),
                "-dPDFX=4".to_string(),
                "-dBATCH".to_string(),
                "-dNOPAUSE".to_string(),
                "-sDEVICE=pdfwrite".to_string(),
                "-sProcessColorModel=DeviceCMYK".to_string(),
                "-sColorConversionStrategy=CMYK".to_string(),
                "-sColorConversionStrategyForImages=CMYK".to_string(),
                "-dOverrideICC=true".to_string(),
                "-dDownsampleColorImages=true".to_string(),
                "-dColorImageResolution=300".to_string(),
                "-dDownsampleGrayImages=true".to_string(),
                "-dGrayImageResolution=300".to_string(),
                format!("-sOutputFile={}", output_pdf),
                self.ps_config_path.to_string_lossy().to_string(),
                input_pdf.to_string(),
            ])
            .output();

        match output {
            Ok(out) if out.status.success() => Ok(()),
            Ok(out) => Err(ImposerError::Pipeline(
                String::from_utf8_lossy(&out.stderr).to_string(),
            )),
            Err(e) => Err(ImposerError::MissingDependency(format!(
                "Ghostscript introuvable : {}",
                e
            ))),
        }
    }
}

pub fn temp_asset_path(name: &str) -> PathBuf {
    env::temp_dir().join(name)
}

#[allow(dead_code)]
pub fn _use_path(_p: PathBuf) {}
