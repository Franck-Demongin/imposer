use crate::config::SheetPreset;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserPreset {
    pub name: String,
    pub width_mm: f32,
    pub height_mm: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserPresetFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub presets: Vec<UserPreset>,
}

fn default_version() -> u32 {
    1
}

pub fn user_presets_path() -> Result<PathBuf, String> {
    let config_dir = platform_config_dir().ok_or_else(|| {
        "impossible de déterminer le dossier de configuration utilisateur".to_string()
    })?;
    Ok(config_dir.join("imposer").join("presets.toml"))
}

pub fn load_user_presets() -> Result<Vec<UserPreset>, String> {
    let path = user_presets_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(&path)
        .map_err(|e| format!("erreur lecture {} : {e}", path.display()))?;
    let parsed: UserPresetFile = toml::from_str(&data)
        .map_err(|e| format!("fichier TOML invalide {} : {e}", path.display()))?;
    Ok(parsed.presets)
}

pub fn save_user_presets(presets: &[UserPreset]) -> Result<(), String> {
    let path = user_presets_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("impossible de créer {} : {e}", parent.display()))?;
    }

    let file = UserPresetFile {
        version: default_version(),
        presets: presets.to_vec(),
    };
    let data = toml::to_string_pretty(&file)
        .map_err(|e| format!("impossible de sérialiser les préréglages : {e}"))?;

    fs::write(&path, data).map_err(|e| format!("impossible d'écrire {} : {e}", path.display()))?;
    Ok(())
}

pub fn validate_user_preset(
    name: &str,
    width_mm: f32,
    height_mm: f32,
    existing: &[UserPreset],
    builtins: &[SheetPreset],
) -> Result<UserPreset, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("le nom du préréglé est vide".to_string());
    }
    if name.chars().count() > 80 {
        return Err("le nom du préréglé est trop long".to_string());
    }
    if !width_mm.is_finite() || !height_mm.is_finite() || width_mm < 1.0 || height_mm < 1.0 {
        return Err("dimensions de préréglé invalides".to_string());
    }
    let lower = name.to_lowercase();
    if builtins.iter().any(|p| p.name.to_lowercase() == lower)
        || existing.iter().any(|p| p.name.to_lowercase() == lower)
    {
        return Err("ce nom de préréglé existe déjà".to_string());
    }
    Ok(UserPreset {
        name: name.to_string(),
        width_mm,
        height_mm,
    })
}

fn platform_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        for variable in ["APPDATA", "LOCALAPPDATA"] {
            if let Ok(value) = std::env::var(variable)
                && !value.trim().is_empty()
            {
                return Some(PathBuf::from(value));
            }
        }
        let user_profile = std::env::var("USERPROFILE").ok()?;
        let mut path = PathBuf::from(user_profile);
        path.push("AppData");
        path.push("Roaming");
        Some(path)
    }

    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").ok()?;
        let mut path = PathBuf::from(home);
        path.push("Library");
        path.push("Application Support");
        Some(path)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
            && !xdg.trim().is_empty()
        {
            return Some(PathBuf::from(xdg));
        }
        let home = std::env::var("HOME").ok()?;
        let mut path = PathBuf::from(home);
        path.push(".config");
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_new_preset() {
        let builtins = crate::config::SheetSize::builtins();
        let preset = validate_user_preset("Second", 210.0, 297.0, &[], &builtins).unwrap();
        assert_eq!(preset.name, "Second");
        assert_eq!(preset.width_mm, 210.0);
        assert_eq!(preset.height_mm, 297.0);
    }

    #[test]
    fn rejects_duplicate_preset_names_case_insensitively() {
        let builtins = crate::config::SheetSize::builtins();
        let existing = vec![UserPreset {
            name: "custom".to_string(),
            width_mm: 100.0,
            height_mm: 100.0,
        }];
        assert!(validate_user_preset("CUSTOM", 150.0, 150.0, &existing, &builtins).is_err());
        assert!(validate_user_preset("A4", 100.0, 100.0, &[], &builtins).is_err());
        assert!(validate_user_preset("   ", 100.0, 100.0, &[], &builtins).is_err());
    }
}
