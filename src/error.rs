use thiserror::Error;

#[derive(Debug, Error)]
pub enum ImposerError {
    #[error("impossible de lire le PDF : {0}")]
    Parse(String),
    #[error("le PDF doit contenir 1 ou 2 pages (trouvé {0})")]
    InvalidPageCount(usize),
    #[error("impossible d'obtenir les dimensions de la page : {0}")]
    Dimensions(String),
    #[error(" aucun placement possible : le document est trop grand pour la planche")]
    NoFit,
    #[error("fond perdu trop grand par rapport au document source : {0}")]
    BleedTooLarge(String),
    #[error("erreur lors de la génération du PDF : {0}")]
    Generation(String),
    #[error("marge de feuille insuffisante pour les traits de coupe : {0}")]
    CropMarksMargin(String),
    #[error("erreur d'entrée/sortie : {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ImposerError>;
