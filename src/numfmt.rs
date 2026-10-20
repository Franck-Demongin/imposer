//! Formatage des nombres dans les flux de contenu PDF.
//!
//! Empêche la notation exponentielle et les valeurs non finies dans les opérandes
//! numériques des flux de contenu PDF/X-4. Normalise le zéro négatif et limite
//! la précision à 4 décimales.
//!
//! Limites connues : cet utilitaire ne remplace pas le codage des matrices
//! complets dans `output`; il ne gère que des scalaires.

use crate::error::ImposerError;

/// Formate un `f64` en opérande sûr pour un flux de contenu PDF.
pub fn format_f64(value: f64) -> Result<String, ImposerError> {
    if !value.is_finite() {
        return Err(ImposerError::Generation(
            "valeur non finie dans le flux de contenu".to_string(),
        ));
    }
    let mut v = if value.abs() < 0.0000001 { 0.0 } else { value };
    // Avoid -0.
    if v == 0.0 {
        v = 0.0_f64;
    }
    // Round to 4 decimals.
    let factor = 10_000.0_f64;
    let rounded = (v * factor).round() / factor;
    let s = format_single(rounded);
    if s.contains('e') || s.contains('E') || s.contains("NaN") || s.contains("inf") {
        return Err(ImposerError::Generation(
            "format de nombre non admissib. dans le flux de contenu".to_string(),
        ));
    }
    Ok(s)
}

fn format_single(v: f64) -> String {
    // Use fixed formatting then trim trailing zeros after decimal.
    let mut s = format!("{:.4}", v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

/// Formate un `f32` après promotion en `f64`.
pub fn format_f32(value: f32) -> Result<String, ImposerError> {
    format_f64(value as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        assert_eq!(format_f64(1.234567).unwrap(), "1.2346");
        assert_eq!(format_f64(0.0).unwrap(), "0");
        assert_eq!(format_f64(-0.0).unwrap(), "0");
        assert_eq!(format_f64(42.0).unwrap(), "42");
    }

    #[test]
    fn rejects_nan_inf() {
        assert!(format_f64(f64::NAN).is_err());
        assert!(format_f64(f64::INFINITY).is_err());
        assert!(format_f64(f64::NEG_INFINITY).is_err());
    }

    #[test]
    fn no_exponent() {
        let s = format_f64(1e-5).unwrap();
        assert!(!s.contains('e'));
        // likely 0.00001 -> 0 after 4 decimals
        assert_eq!(s, "0");
    }
}
