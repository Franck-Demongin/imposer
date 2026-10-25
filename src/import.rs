//! Import d'une page PDF source comme Form XObject (copie profonde).
//!
//! Limite connue : dédoublonnage par hash complet non implémenté ; seules les
//! références identiques sont réutilisées via le cache.

use std::collections::{BTreeMap, HashSet};

use crate::error::{ImposerError, Result};
use crate::icc::IccProfile;
use crate::layout::SheetSpec;
use crate::output::OutputBuilder;
use crate::source::{SourcePdf, inherited_page_dict, page_object_ids};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxSource {
    Bleed,
    Trim,
    Media,
}

#[derive(Debug, Clone)]
pub struct ImportedPage {
    pub form_id: lopdf::ObjectId,
    pub width_pt: f64,
    pub height_pt: f64,
    pub media_box: Option<[f64; 4]>,
    pub crop_box: Option<[f64; 4]>,
    pub bleed_box: Option<[f64; 4]>,
    pub trim_box: Option<[f64; 4]>,
    pub art_box: Option<[f64; 4]>,
    pub rotation_deg: i32,
    pub user_unit: f64,
}

pub fn import_page(
    builder: &mut OutputBuilder,
    src: &SourcePdf,
    page_index: usize,
) -> Result<ImportedPage> {
    let ids = page_object_ids(&src.document);
    let page_id = *ids
        .get(page_index)
        .ok_or_else(|| ImposerError::Parse("page introuvable dans la source".to_string()))?;
    let page = inherited_page_dict(&src.document, page_id)?;

    fn arr4(v: &lopdf::Object) -> Option<[f64; 4]> {
        if let lopdf::Object::Array(a) = v
            && a.len() >= 4
        {
            let x1 = a[0].as_float().ok()? as f64;
            let y1 = a[1].as_float().ok()? as f64;
            let x2 = a[2].as_float().ok()? as f64;
            let y2 = a[3].as_float().ok()? as f64;
            return Some([x1, y1, x2, y2]);
        }
        None
    }
    let media_box = page.get(b"MediaBox").ok().and_then(|v| {
        if let lopdf::Object::Reference(id) = v {
            src.document.get_object(*id).ok().and_then(arr4)
        } else {
            arr4(v)
        }
    });
    let crop_box = page
        .get(b"CropBox")
        .ok()
        .and_then(arr4_from_obj(&src.document));
    let bleed_box = page
        .get(b"BleedBox")
        .ok()
        .and_then(arr4_from_obj(&src.document));
    let trim_box = page
        .get(b"TrimBox")
        .ok()
        .and_then(arr4_from_obj(&src.document));
    let art_box = page
        .get(b"ArtBox")
        .ok()
        .and_then(arr4_from_obj(&src.document));

    let rotate_raw = page
        .get(b"Rotate")
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(0);
    let rotation_deg = ((rotate_raw % 360) + 360) % 360;
    let rotation_deg = rotation_deg as i32;
    let user_unit = page
        .get(b"UserUnit")
        .ok()
        .and_then(|o| o.as_float().ok())
        .map(|v| v as f64)
        .unwrap_or(1.0);

    let mb = media_box.unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let (mw, mh) = (
        ((mb[2] - mb[0]) as f64 * user_unit as f64),
        ((mb[3] - mb[1]) as f64 * user_unit as f64),
    );
    let (width_pt, height_pt) = if rotation_deg == 90 || rotation_deg == 270 {
        (mh, mw)
    } else {
        (mw, mh)
    };

    // Contents concat
    let contents_obj = page.get(b"Contents").cloned().ok();
    let merged = concat_contents(&src.document, contents_obj)?;
    let balanced = balance_qq(&merged);

    // Resources deep copy
    let resources_obj = page
        .get(b"Resources")
        .cloned()
        .unwrap_or(lopdf::Object::Dictionary(lopdf::Dictionary::new()));
    let copied_resources = deep_copy(
        &src.document,
        &mut builder.document,
        &resources_obj,
        &mut builder.resource_cache,
        &mut HashSet::new(),
    )?;

    // Selected BBox
    let selected = match builder.box_source {
        BoxSource::Bleed => bleed_box.or(trim_box).or(media_box),
        BoxSource::Trim => trim_box.or(bleed_box).or(media_box),
        BoxSource::Media => media_box,
    }
    .unwrap_or(mb);
    let bbox = selected;

    // Matrix for rotation
    let matrix = rotation_matrix(rotation_deg, mw, mh);

    // Form dict
    let mut form_dict = lopdf::Dictionary::new();
    form_dict.set("Type", lopdf::Object::Name(b"XObject".to_vec()));
    form_dict.set("Subtype", lopdf::Object::Name(b"Form".to_vec()));
    form_dict.set(
        "BBox",
        lopdf::Object::Array(vec![
            bbox[0].into(),
            bbox[1].into(),
            bbox[2].into(),
            bbox[3].into(),
        ]),
    );
    form_dict.set("Resources", copied_resources);
    form_dict.set(
        "Matrix",
        lopdf::Object::Array(vec![
            matrix[0].into(),
            matrix[1].into(),
            matrix[2].into(),
            matrix[3].into(),
            matrix[4].into(),
            matrix[5].into(),
        ]),
    );

    // Group transparency
    if let Ok(g) = page.get(b"Group") {
        let copied_g = deep_copy(
            &src.document,
            &mut builder.document,
            g,
            &mut builder.resource_cache,
            &mut HashSet::new(),
        )?;
        form_dict.set("Group", copied_g);
    }

    let mut stream = lopdf::Stream::new(form_dict, balanced);
    stream.allows_compression = true;
    let form_id = builder.document.add_object(stream);

    Ok(ImportedPage {
        form_id,
        width_pt,
        height_pt,
        media_box,
        crop_box,
        bleed_box,
        trim_box,
        art_box,
        rotation_deg,
        user_unit: user_unit as f64,
    })
}

fn arr4_from_obj(doc: &lopdf::Document) -> impl Fn(&lopdf::Object) -> Option<[f64; 4]> + '_ {
    move |v| {
        if let lopdf::Object::Reference(id) = v {
            doc.get_object(*id).ok().and_then(arr4_of_obj)
        } else {
            arr4_of_obj(v)
        }
    }
}

fn arr4_of_obj(v: &lopdf::Object) -> Option<[f64; 4]> {
    if let lopdf::Object::Array(a) = v
        && a.len() >= 4
    {
        return Some([
            a[0].as_float().ok()? as f64,
            a[1].as_float().ok()? as f64,
            a[2].as_float().ok()? as f64,
            a[3].as_float().ok()? as f64,
        ]);
    }
    None
}

fn concat_contents(doc: &lopdf::Document, contents: Option<lopdf::Object>) -> Result<Vec<u8>> {
    let mut streams: Vec<lopdf::ObjectId> = Vec::new();
    match contents {
        Some(lopdf::Object::Reference(id)) => streams.push(id),
        Some(lopdf::Object::Array(arr)) => {
            for o in arr {
                if let lopdf::Object::Reference(id) = o {
                    streams.push(id);
                }
            }
        }
        Some(lopdf::Object::Stream(_)) => {
            // inline stream: handle by copying? Not in source usually.
        }
        None => {}
        _ => {}
    }
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for sid in &streams {
        let obj = doc
            .get_object(*sid)
            .map_err(|e| ImposerError::Parse(e.to_string()))?;
        let stream = obj
            .as_stream()
            .map_err(|e| ImposerError::Parse(e.to_string()))?;
        let decompressed = crate::source::decompress_stream(stream)?;
        // If decompressed equals raw (no filter), still okay.
        parts.push(decompressed);
    }
    if parts.is_empty() {
        return Ok(Vec::new());
    }
    let mut merged = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            merged.push(b'\n');
        }
        merged.extend_from_slice(p);
    }
    Ok(merged)
}

fn balance_qq(content: &[u8]) -> Vec<u8> {
    // Count q/Q tokens (whitespace-delimited)
    let mut q = 0i64;
    let mut qq = 0i64;
    for tok in String::from_utf8_lossy(content).split_whitespace() {
        if tok == "q" {
            q += 1;
        } else if tok == "Q" {
            qq += 1;
        }
    }
    let mut out = content.to_vec();
    if q > qq {
        for _ in 0..(q - qq) {
            out.extend_from_slice(b"\nQ");
        }
    } else if qq > q {
        // prepend q
        let mut prefix = Vec::new();
        for _ in 0..(qq - q) {
            prefix.extend_from_slice(b"q\n");
        }
        prefix.extend_from_slice(&out);
        out = prefix;
    }
    out
}

fn rotation_matrix(deg: i32, w: f64, h: f64) -> [f64; 6] {
    match deg {
        90 => [0.0, 1.0, -1.0, 0.0, w, 0.0],
        180 => [-1.0, 0.0, 0.0, -1.0, w, h],
        270 => [0.0, -1.0, 1.0, 0.0, 0.0, h],
        _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    }
}

// ------ deep copy with remapping & cycle handling ------
fn deep_copy(
    src: &lopdf::Document,
    dest: &mut lopdf::Document,
    obj: &lopdf::Object,
    cache: &mut BTreeMap<(u32, u16), lopdf::ObjectId>,
    visited: &mut HashSet<lopdf::ObjectId>,
) -> Result<lopdf::Object> {
    use lopdf::Object;
    match obj {
        Object::Reference(id) => {
            let key = (id.0, id.1);
            if let Some(&new_id) = cache.get(&key) {
                return Ok(Object::Reference(new_id));
            }
            if visited.contains(id) {
                // cycle -> null
                return Ok(Object::Null);
            }
            // get object
            let referenced = match src.get_object(*id) {
                Ok(o) => o.clone(),
                Err(_) => return Ok(Object::Null),
            };
            visited.insert(*id);
            let copied = deep_copy(src, dest, &referenced, cache, visited)?;
            visited.remove(id);
            let new_id = dest.add_object(copied);
            cache.insert(key, new_id);
            Ok(Object::Reference(new_id))
        }
        Object::Dictionary(dict) => {
            let mut new_dict = lopdf::Dictionary::new();
            for (key, value) in dict {
                // skip dangerous keys
                if should_skip_key(key) {
                    continue;
                }
                let cv = deep_copy(src, dest, value, cache, visited)?;
                new_dict.set(key.clone(), cv);
            }
            Ok(Object::Dictionary(new_dict))
        }
        Object::Array(arr) => {
            let mut new_arr = Vec::with_capacity(arr.len());
            for item in arr {
                new_arr.push(deep_copy(src, dest, item, cache, visited)?);
            }
            Ok(Object::Array(new_arr))
        }
        Object::Stream(stream) => {
            let mut new_dict = lopdf::Dictionary::new();
            for (key, value) in &stream.dict {
                if should_skip_key(key) {
                    continue;
                }
                let cv = deep_copy(src, dest, value, cache, visited)?;
                new_dict.set(key.clone(), cv);
            }
            Ok(Object::Stream(lopdf::Stream {
                dict: new_dict,
                content: stream.content.clone(),
                allows_compression: stream.allows_compression,
                start_position: None,
            }))
        }
        other => Ok(other.clone()),
    }
}

fn should_skip_key(key: &[u8]) -> bool {
    matches!(
        key,
        b"Parent" | b"P" | b"StructParents" | b"Annots" | b"B" | b"Thumb"
    )
}

// helper to avoid unused import warnings
#[allow(unused)]
fn _use_sheet_spec(_s: SheetSpec) {}
#[allow(unused)]
fn _use_icc(_p: IccProfile) {}

pub fn import_raster(
    builder: &mut OutputBuilder,
    source: &crate::source::SourcePage,
) -> Result<ImportedPage> {
    use crate::error::ImposerError;

    match &source.kind {
        crate::source::SourceKind::Raster {
            pixels,
            width_px,
            height_px,
            color_space,
            icc_profile,
        } => {
            if *width_px == 0 || *height_px == 0 {
                return Err(ImposerError::Generation(
                    "dimensions raster invalides".to_string(),
                ));
            }

            let color_space_obj = match color_space {
                crate::source::ImageColorSpace::DeviceGray => {
                    lopdf::Object::Name(b"DeviceGray".to_vec())
                }
                crate::source::ImageColorSpace::DeviceRgb => {
                    lopdf::Object::Name(b"DeviceRGB".to_vec())
                }
                crate::source::ImageColorSpace::DeviceCmyk => {
                    let profile = icc_profile.as_deref().ok_or(ImposerError::NoProfile)?;
                    let profile_bytes = profile.to_vec();
                    let mut icc_dict = lopdf::Dictionary::new();
                    icc_dict.set("N", lopdf::Object::Integer(4));
                    icc_dict.set("Length", lopdf::Object::Integer(profile_bytes.len() as i64));
                    let id = builder.document.add_object(lopdf::Stream {
                        dict: icc_dict,
                        content: profile_bytes,
                        allows_compression: false,
                        start_position: None,
                    });
                    lopdf::Object::Array(vec![
                        lopdf::Object::Name(b"ICCBased".to_vec()),
                        lopdf::Object::Reference(id),
                    ])
                }
            };

            let mut image_dict = lopdf::Dictionary::new();
            image_dict.set("Type", lopdf::Object::Name(b"XObject".to_vec()));
            image_dict.set("Subtype", lopdf::Object::Name(b"Image".to_vec()));
            image_dict.set("Width", lopdf::Object::Integer(*width_px as i64));
            image_dict.set("Height", lopdf::Object::Integer(*height_px as i64));
            image_dict.set("ColorSpace", color_space_obj);
            image_dict.set("BitsPerComponent", lopdf::Object::Integer(8));

            image_dict.set("Length", lopdf::Object::Integer(pixels.len() as i64));
            let mut image_stream = lopdf::Stream::new(image_dict, pixels.clone());
            image_stream.allows_compression = true;
            let image_id = builder.document.add_object(image_stream);

            let width_pt = source.width_pt as f64;
            let height_pt = source.height_pt as f64;
            let content = format!(
                "q {width_pt} 0 0 {height_pt} 0 0 cm /Img Do Q
"
            );
            let mut form_dict = lopdf::Dictionary::new();
            form_dict.set("Type", lopdf::Object::Name(b"XObject".to_vec()));
            form_dict.set("Subtype", lopdf::Object::Name(b"Form".to_vec()));
            form_dict.set(
                "BBox",
                lopdf::Object::Array(vec![
                    0.0.into(),
                    0.0.into(),
                    width_pt.into(),
                    height_pt.into(),
                ]),
            );

            let mut xobjects = lopdf::Dictionary::new();
            xobjects.set(b"Img".to_vec(), lopdf::Object::Reference(image_id));
            let mut resources = lopdf::Dictionary::new();
            resources.set(b"XObject".to_vec(), lopdf::Object::Dictionary(xobjects));
            form_dict.set("Resources", lopdf::Object::Dictionary(resources));

            let form_bytes = content.into_bytes();
            form_dict.set("Length", lopdf::Object::Integer(form_bytes.len() as i64));
            let mut form_stream = lopdf::Stream::new(form_dict, form_bytes);
            form_stream.allows_compression = true;
            let form_id = builder.document.add_object(form_stream);

            Ok(ImportedPage {
                form_id,
                width_pt,
                height_pt,
                media_box: None,
                crop_box: None,
                bleed_box: None,
                trim_box: None,
                art_box: None,
                rotation_deg: 0,
                user_unit: 1.0,
            })
        }
        _ => Err(ImposerError::Generation(
            "source raster attendue".to_string(),
        )),
    }
}

fn _keep() {}
