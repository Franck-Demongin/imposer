use clap::ValueEnum;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SheetOrientation {
    Auto,
    Portrait,
    Landscape,
}

#[derive(Debug, Clone, Copy)]
pub struct ImpositionConfig {
    pub sheet_size: SheetSize,
    pub sheet_orientation: SheetOrientation,
    pub bleed_mm: f32,
    pub safety_margin_mm: f32,
    pub rotate_verso: bool,
    pub rotate_copies: bool,
}

impl Default for ImpositionConfig {
    fn default() -> Self {
        Self {
            sheet_size: SheetSize::A4,
            sheet_orientation: SheetOrientation::Auto,
            bleed_mm: 2.0,
            safety_margin_mm: 5.0,
            rotate_verso: false,
            rotate_copies: true,
        }
    }
}
