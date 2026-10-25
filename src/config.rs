use crate::error::{ImposerError, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SheetSize {
    A4,
    A3,
    Sra3,
}

impl SheetSize {
    /// Dimensions (largeur, hauteur) en millimètres, orientation portrait.
    pub fn dimensions_mm(self) -> (f32, f32) {
        match self {
            SheetSize::A4 => (210.0, 297.0),
            SheetSize::A3 => (297.0, 420.0),
            SheetSize::Sra3 => (320.0, 450.0),
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            SheetSize::A4 => "A4",
            SheetSize::A3 => "A3",
            SheetSize::Sra3 => "SRA3",
        }
    }

    /// Préréglés intégrés, toujours disponibles sans fichier de préférences.
    pub fn builtins() -> Vec<SheetPreset> {
        [SheetSize::A4, SheetSize::A3, SheetSize::Sra3]
            .into_iter()
            .map(|size| {
                let (width_mm, height_mm) = size.dimensions_mm();
                SheetPreset {
                    name: size.name().to_string(),
                    width_mm,
                    height_mm,
                    builtin: true,
                    editable: false,
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SheetOrientation {
    Auto,
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Copy)]
pub struct ImpositionConfig {
    pub sheet_width_mm: f32,
    pub sheet_height_mm: f32,
    pub sheet_orientation: SheetOrientation,
    pub bleed_mm: f32,
    pub safety_margin_mm: f32,
    pub rotate_verso: bool,
    pub rotate_copies: bool,
}

impl Default for ImpositionConfig {
    fn default() -> Self {
        Self {
            sheet_width_mm: 210.0,
            sheet_height_mm: 297.0,
            sheet_orientation: SheetOrientation::Auto,
            bleed_mm: 2.0,
            safety_margin_mm: 5.0,
            rotate_verso: false,
            rotate_copies: true,
        }
    }
}

impl ImpositionConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.sheet_width_mm.is_finite() || self.sheet_width_mm < 1.0 {
            return Err(ImposerError::Generation(
                "largeur de feuille invalide".to_string(),
            ));
        }
        if !self.sheet_height_mm.is_finite() || self.sheet_height_mm < 1.0 {
            return Err(ImposerError::Generation(
                "hauteur de feuille invalide".to_string(),
            ));
        }
        if !self.bleed_mm.is_finite() || self.bleed_mm < 0.0 {
            return Err(ImposerError::Generation(
                "bord perdu négatif ou invalide".to_string(),
            ));
        }
        if !self.safety_margin_mm.is_finite() || self.safety_margin_mm < 0.0 {
            return Err(ImposerError::Generation(
                "marge de sécurité négative ou invalide".to_string(),
            ));
        }
        if self.bleed_mm >= self.sheet_width_mm / 2.0 || self.bleed_mm >= self.sheet_height_mm / 2.0
        {
            return Err(ImposerError::Generation(
                "bord perdu supérieur à la moitié de la feuille".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetPreset {
    pub name: String,
    pub width_mm: f32,
    pub height_mm: f32,
    pub builtin: bool,
    #[serde(default)]
    pub editable: bool,
}

impl SheetPreset {
    pub fn cm_label(&self) -> String {
        format!(
            "{} — {:.1} x {:.1} cm",
            self.name,
            self.width_mm / 10.0,
            self.height_mm / 10.0
        )
    }
}
