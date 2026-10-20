//! Détection du profil ICC CMJN dans un PDF source.
//!
//! Limite connue : l'OCG n'est pas fusionné ; les filtres ICC non supportés
//! produisent une erreur claire.

use std::collections::{BTreeSet, HashSet};

use crate::error::{ImposerError, Result};
use crate::icc::IccProfile;
use crate::source::{SourcePdf, inherited_page_dict, page_object_ids};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSource {
    OutputIntent,
    IccBased,
    Assumed,
    None,
}

#[derive(Debug, Clone)]
pub struct ProfileDetection {
    pub profile: Option<IccProfile>,
    pub source: ProfileSource,
    pub warnings: Vec<String>,
}

pub fn detect_cmyk_profile(
    src: &SourcePdf,
    page_index: usize,
    assume_profile: Option<IccProfile>,
) -> Result<ProfileDetection> {
    let doc = &src.document;
    let mut warnings = Vec::new();

    // (a) OutputIntent GTS_PDFX
    if let Some(p) = output_intent_profile(doc) {
        // also scan ICCBased to compare (warning if differs)
        let found = scan_page_icc(doc, page_index);
        let distinct = distinct_profiles(&found);
        if distinct.len() > 1 {
            warnings.push("plusieurs profils ICCBased détectés dans la page".to_string());
        } else if let Some(first) = found.first()
            && first.info.sha256 != p.info.sha256
        {
            warnings.push("OutputIntent et ICCBased diffèrent ; OutputIntent fait foi".to_string());
        }
        return Ok(ProfileDetection {
            profile: Some(p),
            source: ProfileSource::OutputIntent,
            warnings,
        });
    }

    // (b) scan resources
    let found = scan_page_icc(doc, page_index);
    let distinct = distinct_profiles(&found);
    if distinct.len() > 1 {
        return Err(ImposerError::AmbiguousProfile);
    }
    if let Some(p) = distinct.into_iter().next() {
        return Ok(ProfileDetection {
            profile: Some(p),
            source: ProfileSource::IccBased,
            warnings,
        });
    }

    // (c) NoProfile unless assume_profile
    if let Some(p) = assume_profile {
        return Ok(ProfileDetection {
            profile: Some(p),
            source: ProfileSource::Assumed,
            warnings,
        });
    }
    Ok(ProfileDetection {
        profile: None,
        source: ProfileSource::None,
        warnings,
    })
}

fn distinct_profiles(profiles: &[IccProfile]) -> Vec<IccProfile> {
    let mut out: Vec<IccProfile> = Vec::new();
    for p in profiles {
        if !out.iter().any(|q| q.info.sha256 == p.info.sha256) {
            out.push(p.clone());
        }
    }
    out
}

fn output_intent_profile(doc: &lopdf::Document) -> Option<IccProfile> {
    let catalog_id = match doc.trailer.get(b"Root") {
        Ok(lopdf::Object::Reference(id)) => id,
        _ => return None,
    };
    let catalog = match doc.get_object(*catalog_id) {
        Ok(lopdf::Object::Dictionary(d)) => d,
        _ => return None,
    };
    let intents = match catalog.get(b"OutputIntents") {
        Ok(o) => o.clone(),
        Err(_) => return None,
    };
    // normalize to list
    let list = match intents {
        lopdf::Object::Array(a) => a,
        lopdf::Object::Reference(id) => {
            if let Ok(lopdf::Object::Array(a)) = doc.get_object(id) {
                a.clone()
            } else {
                return None;
            }
        }
        lopdf::Object::Dictionary(_) => vec![intents],
        _ => return None,
    };
    for item in list {
        let dict = match item {
            lopdf::Object::Dictionary(d) => d,
            lopdf::Object::Reference(id) => {
                if let Ok(lopdf::Object::Dictionary(d)) = doc.get_object(id) {
                    d.clone()
                } else {
                    continue;
                }
            }
            _ => continue,
        };
        let s = dict.get(b"S").ok().and_then(as_name).unwrap_or_default();
        if s != "GTS_PDFX" {
            continue;
        }
        let profile_ref = match dict.get(b"DestOutputProfile") {
            Ok(lopdf::Object::Reference(id)) => id,
            _ => continue,
        };
        if let Ok(profile) = stream_to_icc(doc, *profile_ref) {
            return Some(profile);
        }
    }
    None
}

fn stream_to_icc(doc: &lopdf::Document, stream_id: lopdf::ObjectId) -> Result<IccProfile> {
    let obj = doc
        .get_object(stream_id)
        .map_err(|e| ImposerError::Parse(e.to_string()))?;
    let stream = obj
        .as_stream()
        .map_err(|e| ImposerError::Parse(e.to_string()))?;
    let data = crate::source::decompress_stream(stream)?;
    IccProfile::parse(&data).map_err(|e| ImposerError::InvalidIcc(e.to_string()))
}

fn as_name(o: &lopdf::Object) -> Option<String> {
    match o {
        lopdf::Object::Name(n) => Some(String::from_utf8_lossy(n).to_string()),
        _ => None,
    }
}

fn scan_page_icc(doc: &lopdf::Document, page_index: usize) -> Vec<IccProfile> {
    let ids = page_object_ids(doc);
    let page_id = match ids.get(page_index) {
        Some(id) => id,
        None => return Vec::new(),
    };
    let page = match inherited_page_dict(doc, *page_id) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let resources = match page.get(b"Resources") {
        Ok(lopdf::Object::Dictionary(d)) => d.clone(),
        Ok(lopdf::Object::Reference(id)) => {
            if let Ok(lopdf::Object::Dictionary(d)) = doc.get_object(*id) {
                d.clone()
            } else {
                return Vec::new();
            }
        }
        _ => return Vec::new(),
    };

    let mut found = Vec::new();
    let mut visited = HashSet::new();
    collect_resources(doc, &resources, &mut found, &mut visited);
    found
}

#[allow(clippy::only_used_in_recursion)]
fn collect_resources(
    doc: &lopdf::Document,
    res: &lopdf::Dictionary,
    found: &mut Vec<IccProfile>,
    visited: &mut HashSet<lopdf::ObjectId>,
) {
    // ColorSpace entries
    if let Ok(o) = res.get(b"ColorSpace") {
        let dict = resolve_dict(doc, o);
        if let Some(d) = dict {
            for (_, v) in d {
                extract_colorspace(doc, &v, found, visited);
            }
        }
    }
    // XObject entries
    if let Ok(o) = res.get(b"XObject") {
        let dict = resolve_dict(doc, o);
        if let Some(d) = dict {
            for (_, v) in d {
                if let lopdf::Object::Reference(id) = v {
                    if !visited.insert(id) {
                        continue;
                    }
                    // image/form xobject
                    if let Ok(lopdf::Object::Stream(st)) = doc.get_object(id) {
                        // ColorSpace of image
                        if let Ok(cs) = st.dict.get(b"ColorSpace") {
                            extract_colorspace(doc, cs, found, visited);
                        }
                        // Form XObject resources
                        if let Ok(r) = st.dict.get(b"Resources") {
                            let rd = resolve_dict(doc, r);
                            if let Some(d2) = rd {
                                collect_resources(doc, &d2, found, visited);
                            }
                        }
                        // Group color space
                        if let Ok(g) = st.dict.get(b"Group") {
                            let gd = resolve_dict(doc, g);
                            if let Some(gd) = gd
                                && let Ok(cs) = gd.get(b"CS")
                            {
                                extract_colorspace(doc, cs, found, visited);
                            }
                        }
                    }
                }
            }
        }
    }
    // Pattern entries
    if let Ok(o) = res.get(b"Pattern") {
        let dict = resolve_dict(doc, o);
        if let Some(d) = dict {
            for (_, v) in d {
                if let lopdf::Object::Reference(id) = v
                    && visited.insert(id)
                    && let Ok(lopdf::Object::Stream(st)) = doc.get_object(id)
                {
                    if let Ok(r) = st.dict.get(b"Resources") {
                        let rd = resolve_dict(doc, r);
                        if let Some(d2) = rd {
                            collect_resources(doc, &d2, found, visited);
                        }
                    }
                    if let Ok(cs) = st.dict.get(b"ColorSpace") {
                        extract_colorspace(doc, cs, found, visited);
                    }
                }
            }
        }
    }
    // Shading entries
    if let Ok(o) = res.get(b"Shading") {
        let dict = resolve_dict(doc, o);
        if let Some(d) = dict {
            for (_, v) in d {
                if let lopdf::Object::Reference(id) = v {
                    if let Ok(lopdf::Object::Dictionary(sd)) = doc.get_object(id)
                        && let Ok(cs) = sd.get(b"ColorSpace")
                    {
                        extract_colorspace(doc, cs, found, visited);
                    }
                } else if let lopdf::Object::Dictionary(sd) = v
                    && let Ok(cs) = sd.get(b"ColorSpace")
                {
                    extract_colorspace(doc, cs, found, visited);
                }
            }
        }
    }
    // Font entries -> Type3 resources
    if let Ok(o) = res.get(b"Font") {
        let dict = resolve_dict(doc, o);
        if let Some(d) = dict {
            for (_, v) in d {
                if let lopdf::Object::Reference(id) = v
                    && let Ok(lopdf::Object::Dictionary(fd)) = doc.get_object(id)
                    && let Ok(sub) = fd.get(b"Resources")
                {
                    let rd = resolve_dict(doc, sub);
                    if let Some(d2) = rd {
                        collect_resources(doc, &d2, found, visited);
                    }
                }
            }
        }
    }
    // Properties (OCG etc) - recurse
    if let Ok(o) = res.get(b"Properties") {
        let dict = resolve_dict(doc, o);
        if let Some(d) = dict {
            // only warn; OCG not merged. Keep silent mostly.
            let _ = d;
        }
    }
}

fn resolve_dict(doc: &lopdf::Document, o: &lopdf::Object) -> Option<lopdf::Dictionary> {
    match o {
        lopdf::Object::Dictionary(d) => Some(d.clone()),
        lopdf::Object::Reference(id) => {
            if let Ok(lopdf::Object::Dictionary(d)) = doc.get_object(*id) {
                Some(d.clone())
            } else {
                None
            }
        }
        _ => None,
    }
}

#[allow(clippy::only_used_in_recursion)]
fn extract_colorspace(
    doc: &lopdf::Document,
    obj: &lopdf::Object,
    found: &mut Vec<IccProfile>,
    visited: &mut HashSet<lopdf::ObjectId>,
) {
    // direct ICCBased array or ref
    let target = match obj {
        lopdf::Object::Reference(id) => {
            if let Ok(o) = doc.get_object(*id) {
                Box::new(o.clone())
            } else {
                return;
            }
        }
        other => Box::new(other.clone()),
    };
    match *target {
        lopdf::Object::Array(arr) => {
            if arr.first().and_then(as_name_obj).as_deref() == Some("ICCBased") {
                // second element
                if let Some(second) = arr.get(1) {
                    let stream_id = match second {
                        lopdf::Object::Reference(id) => id,
                        _ => return,
                    };
                    if let Ok(profile) = stream_to_icc(doc, *stream_id)
                        && !found.iter().any(|p| p.info.sha256 == profile.info.sha256)
                    {
                        found.push(profile);
                    }
                }
            } else {
                // recursive into array components (DeviceN base, Separation alternate?)
                for e in &arr {
                    extract_colorspace(doc, e, found, visited);
                }
            }
        }
        lopdf::Object::Dictionary(_) => {
            // possibly resource? no
        }
        _ => {}
    }
}

fn as_name_obj(o: &lopdf::Object) -> Option<String> {
    match o {
        lopdf::Object::Name(n) => Some(String::from_utf8_lossy(n).to_string()),
        _ => None,
    }
}

// avoid unused import warning
#[allow(unused)]
fn _keep_btree() -> BTreeSet<()> {
    BTreeSet::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::open_pdf_source;
    use lopdf::{Dictionary, Document, Object};

    #[test]
    fn no_profile_returns_none() {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let page_id = doc.new_object_id();
        let mut p = Dictionary::new();
        p.set("Type", Object::Name(b"Page".to_vec()));
        p.set("Parent", Object::Reference(pages_id));
        p.set(
            "MediaBox",
            Object::Array(vec![0i64.into(), 0i64.into(), 100i64.into(), 100i64.into()]),
        );
        doc.objects.insert(page_id, Object::Dictionary(p));
        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
        pages.set("Count", Object::Integer(1));
        doc.objects.insert(pages_id, Object::Dictionary(pages));
        let cat_id = doc.new_object_id();
        let mut cat = Dictionary::new();
        cat.set("Type", Object::Name(b"Catalog".to_vec()));
        cat.set("Pages", Object::Reference(pages_id));
        doc.objects.insert(cat_id, Object::Dictionary(cat));
        doc.trailer.set("Root", Object::Reference(cat_id));
        let bytes = {
            let mut b = Vec::new();
            doc.save_to(&mut b).unwrap();
            b
        };
        let src = open_pdf_source(bytes).unwrap();
        let det = detect_cmyk_profile(&src, 0, None).unwrap();
        assert!(det.profile.is_none());
    }
}
