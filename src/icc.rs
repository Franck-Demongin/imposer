//! Analyse et comparaison des profils ICC (en-tête de 128 octets, sans dépendance C).
//!
//! Limite connue : la description textuelle est extraite si présente sous le tag
//! `desc` ; sinon le champ vaut `None`. Aucune validation colorimétrique fine.

use sha2::{Digest, Sha256};

use crate::error::ImposerError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileMatch {
    Identical,
    SameProfileId,
    SameDescription,
    Different,
}

#[derive(Debug, Clone)]
pub struct IccInfo {
    pub version_major: u8,
    pub color_space: String,
    pub device_class: String,
    pub pcs: String,
    pub description: Option<String>,
    pub profile_id: [u8; 16],
    pub sha256: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct IccProfile {
    pub bytes: Vec<u8>,
    pub info: IccInfo,
    pub warnings: Vec<String>,
}

impl IccProfile {
    pub fn parse(bytes: &[u8]) -> Result<IccProfile, ImposerError> {
        if bytes.len() < 128 {
            return Err(ImposerError::Parse(
                "profil ICC tronqué (< 128 octets)".to_string(),
            ));
        }
        // Profile size declared at 0..4 must equal actual length.
        let declared = u32::from_be_bytes(bytes[0..4].try_into().unwrap_or([0; 4])) as usize;
        if declared != bytes.len() {
            return Err(ImposerError::Parse(format!(
                "taille ICC déclarée ({declared}) != longueur réelle ({})",
                bytes.len()
            )));
        }
        // Signature 'acsp' at 36..40.
        if &bytes[36..40] != b"acsp" {
            return Err(ImposerError::Parse(
                "signature ICC 'acsp' absente".to_string(),
            ));
        }
        // Color space at 16..20 == CMYK.
        let color_space = fourcc(&bytes[16..20]);
        if color_space != "CMYK" {
            return Err(ImposerError::Parse(format!(
                "espace colorimétrique ICC non CMJN : {color_space}"
            )));
        }
        let device_class = fourcc(&bytes[12..16]);
        let pcs = fourcc(&bytes[20..24]);
        if pcs != "Lab " && pcs != "XYZ " {
            return Err(ImposerError::Parse(format!("PCS ICC invalide : {pcs:?}")));
        }
        let version_major = bytes[8];
        if version_major != 2 && version_major != 4 {
            return Err(ImposerError::Parse(format!(
                "version ICC majeure non supportée : {version_major}"
            )));
        }

        let mut warnings = Vec::new();
        if device_class != "prtr" {
            warnings.push(format!(
                "classe de périphérique ICC inattendue : {device_class} (attendu prtr)"
            ));
        }
        if version_major != 2 && version_major != 4 {
            // already rejected; retained for symmetry
        }
        // Recompute version warning not needed here (rejected above).

        let profile_id = {
            let mut arr = [0u8; 16];
            arr.copy_from_slice(&bytes[84..100]);
            arr
        };

        let sha = Sha256::digest(bytes);
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&sha);

        let description = extract_desc(bytes);

        let info = IccInfo {
            version_major,
            color_space,
            device_class,
            pcs,
            description,
            profile_id,
            sha256,
        };

        Ok(IccProfile {
            bytes: bytes.to_vec(),
            info,
            warnings,
        })
    }

    pub fn profile_id_nonzero(&self) -> bool {
        self.info.profile_id.iter().any(|b| *b != 0)
    }

    pub fn sha256_hex(&self) -> String {
        hex::encode(self.info.sha256)
    }

    pub fn compare(&self, other: &IccProfile) -> ProfileMatch {
        if self.bytes == other.bytes {
            return ProfileMatch::Identical;
        }
        if self.profile_id_nonzero() && self.info.profile_id == other.info.profile_id {
            return ProfileMatch::SameProfileId;
        }
        if !profile_id_zero_fallback(self) && !profile_id_zero_fallback(other) {
            // both ids zero: fall back to hash (handled below)
        }
        // ID often null: compare hash.
        let both_zero = !self.profile_id_nonzero() && !other.profile_id_nonzero();
        if both_zero && self.info.sha256 == other.info.sha256 {
            return ProfileMatch::Identical;
        }
        if let (Some(a), Some(b)) = (&self.info.description, &other.info.description)
            && !a.is_empty()
            && a == b
        {
            return ProfileMatch::SameDescription;
        }
        ProfileMatch::Different
    }
}

pub fn profile_id_zero_fallback(_p: &IccProfile) -> bool {
    // reserved
    false
}

fn fourcc(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_string()
}

fn extract_desc(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 132 {
        return None;
    }
    let tag_count = u32::from_be_bytes(bytes[128..132].try_into().ok()?) as usize;
    let mut offset = 132usize;
    for _ in 0..tag_count {
        if offset + 12 > bytes.len() {
            break;
        }
        let sig = &bytes[offset..offset + 4];
        let o = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().ok()?) as usize;
        let size = u32::from_be_bytes(bytes[offset + 8..offset + 12].try_into().ok()?) as usize;
        if sig == b"desc" && o + 12 <= bytes.len() {
            // parse desc tag: type(4) reserved(4) length(4) text...
            if o + 12 <= bytes.len() {
                let ttype = &bytes[o..o + 4];
                if ttype == b"desc" && size >= 12 {
                    let len = u32::from_be_bytes(bytes[o + 8..o + 12].try_into().ok()?) as usize;
                    if o + 12 + len <= bytes.len() {
                        let txt = &bytes[o + 12..o + 12 + len];
                        // strip trailing null
                        let mut t = txt.to_vec();
                        while t.last() == Some(&0) {
                            t.pop();
                        }
                        return String::from_utf8(t).ok();
                    }
                }
            }
        }
        offset += 12;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(cspace: &[u8], class: &[u8], major: u8) -> Vec<u8> {
        let mut b = vec![0u8; 256];
        // size
        let sz = b.len() as u32;
        b[0..4].copy_from_slice(&sz.to_be_bytes());
        // version
        b[8] = major;
        // device class 12..16
        b[12..16].copy_from_slice(class);
        // color space 16..20
        b[16..20].copy_from_slice(cspace);
        // pcs 20..24
        b[20..24].copy_from_slice(b"Lab ");
        // signature
        b[36..40].copy_from_slice(b"acsp");
        // profile id zeros initially
        b
    }

    #[test]
    fn parse_valid() {
        let b = header(b"CMYK", b"prtr", 4);
        let p = IccProfile::parse(&b).unwrap();
        assert_eq!(p.info.color_space, "CMYK");
        assert_eq!(p.info.version_major, 4);
    }

    #[test]
    fn reject_bad_signature() {
        let mut b = header(b"CMYK", b"prtr", 4);
        b[36..40].copy_from_slice(b"nope");
        assert!(IccProfile::parse(&b).is_err());
    }

    #[test]
    fn reject_bad_size() {
        let mut b = header(b"CMYK", b"prtr", 4);
        b[0..4].copy_from_slice(&100u32.to_be_bytes());
        assert!(IccProfile::parse(&b).is_err());
    }

    #[test]
    fn reject_non_cmyk() {
        let b = header(b"RGB ", b"prtr", 4);
        assert!(IccProfile::parse(&b).is_err());
    }

    #[test]
    fn compare_identical() {
        let b = header(b"CMYK", b"prtr", 4);
        let a = IccProfile::parse(&b).unwrap();
        let c = IccProfile::parse(&b).unwrap();
        assert_eq!(a.compare(&c), ProfileMatch::Identical);
    }
}
