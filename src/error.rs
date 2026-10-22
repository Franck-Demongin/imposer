use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
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
    #[error("conflit de couleur des sources : {0}")]
    MixedSourceColors(String),
    #[error("profils ICC des sources incompatibles : {0}")]
    IncompatibleProfiles(String),
    #[error("aucun profil ICC CMJN détecté")]
    NoProfile,
    #[error("plusieurs profils ICC CMJN distincts détectés")]
    AmbiguousProfile,
    #[error("page source chiffree non prise en charge")]
    EncryptedPdf,
    #[error("flux ICC compressé non supporté : {0}")]
    UnsupportedFilter(String),
    #[error("boîtes invalides : {0}")]
    InvalidBoxes(String),
    #[error("placement hors feuille : {0}")]
    OutOfSheet(String),
    #[error("profil ICC invalide : {0}")]
    InvalidIcc(String),
    #[error("cycle détecté lors de la copie d'objet PDF")]
    CycleDetected,
    #[error("référence pendante tolérée, remplacée par null")]
    DanglingReference,
    #[error("{0}")]
    Preflight(String),
}

pub type Result<T> = std::result::Result<T, ImposerError>;
