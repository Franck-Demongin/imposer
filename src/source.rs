use crate::error::{ImposerError, Result};
use crate::icc::IccProfile;
use lopdf::Document;
use psd::Psd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceColorKind {
    Rgb,
    Cmyk,
}

impl SourceColorKind {
    pub fn label(&self) -> &'static str {
        match self {
            SourceColorKind::Rgb => "RVB",
            SourceColorKind::Cmyk => "CMJN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageColorSpace {
    DeviceGray,
    DeviceRgb,
    DeviceCmyk,
}

#[derive(Debug, Clone)]
pub enum SourceKind {
    PdfPage {
        bytes: Vec<u8>,
        page_index: usize,
    },
    Raster {
        pixels: Vec<u8>,
        width_px: u32,
        height_px: u32,
        color_space: ImageColorSpace,
        icc_profile: Option<Vec<u8>>,
    },
}

impl SourcePage {
    pub fn color_kind(&self) -> SourceColorKind {
        match &self.kind {
            SourceKind::PdfPage { bytes, page_index } => pdf_color_kind(bytes, *page_index),
            SourceKind::Raster { color_space, .. } => match color_space {
                ImageColorSpace::DeviceRgb | ImageColorSpace::DeviceGray => SourceColorKind::Rgb,
                ImageColorSpace::DeviceCmyk => SourceColorKind::Cmyk,
            },
        }
    }

    pub fn color_label(&self) -> String {
        match &self.kind {
            SourceKind::Raster {
                color_space,
                icc_profile,
                ..
            } => match color_space {
                ImageColorSpace::DeviceRgb => "RVB (pas de conversion)".to_string(),
                ImageColorSpace::DeviceGray => "RVB (niveaux de gris)".to_string(),
                ImageColorSpace::DeviceCmyk => icc_profile
                    .as_ref()
                    .and_then(|b| IccProfile::parse(b).ok())
                    .map(|p| {
                        if let Some(desc) = p.info.description.as_ref().filter(|s| !s.is_empty()) {
                            format!("CMJN ({desc})")
                        } else {
                            "CMJN (profil ICC embarqué)".to_string()
                        }
                    })
                    .unwrap_or_else(|| "CMJN (sans profil)".to_string()),
            },
            SourceKind::PdfPage { bytes, page_index } => pdf_color_label(bytes, *page_index),
        }
    }

    pub fn cmyk_profile(&self) -> Option<IccProfile> {
        match &self.kind {
            SourceKind::Raster {
                color_space,
                icc_profile,
                ..
            } => match color_space {
                ImageColorSpace::DeviceCmyk => icc_profile
                    .as_deref()
                    .and_then(|b| IccProfile::parse(b).ok()),
                _ => None,
            },
            SourceKind::PdfPage { bytes, page_index } => {
                let src = open_pdf_source(bytes.clone()).ok()?;
                crate::profile::detect_cmyk_profile(&src, *page_index, None)
                    .ok()?
                    .profile
            }
        }
    }

    pub fn cmyk_profile_bytes(&self) -> Option<Vec<u8>> {
        self.cmyk_profile().map(|p| p.bytes)
    }
}

#[derive(Debug, Clone)]
pub struct SourcePage {
    pub width_pt: f32,
    pub height_pt: f32,
    pub kind: SourceKind,
}

pub fn sources_from_bytes(inputs: &[Vec<u8>]) -> Result<Vec<SourcePage>> {
    let mut out = Vec::new();
    for b in inputs {
        out.extend(parse_one(b)?);
    }
    Ok(out)
}

fn parse_one(bytes: &[u8]) -> Result<Vec<SourcePage>> {
    if is_pdf(bytes) {
        parse_pdf(bytes)
    } else if is_png(bytes) {
        Ok(vec![parse_png(bytes)?])
    } else if is_jpeg(bytes) {
        Ok(vec![parse_jpeg(bytes)?])
    } else if is_psd(bytes) {
        Ok(vec![parse_psd(bytes)?])
    } else {
        Err(ImposerError::Parse(
            "format d'entrée non supporté (attendu PDF, PNG, JPEG ou PSD)".to_string(),
        ))
    }
}

fn is_pdf(b: &[u8]) -> bool {
    b.starts_with(b"%PDF")
}
fn is_png(b: &[u8]) -> bool {
    b.len() >= 8 && b[0..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
}
fn is_jpeg(b: &[u8]) -> bool {
    b.len() >= 3 && b[0] == 0xFF && b[1] == 0xD8 && b[2] == 0xFF
}
fn is_psd(b: &[u8]) -> bool {
    b.starts_with(b"8BPS")
}

fn parse_pdf(bytes: &[u8]) -> Result<Vec<SourcePage>> {
    let doc = Document::load_mem(bytes).map_err(|e| ImposerError::Parse(e.to_string()))?;
    let mut pages: Vec<(u32, lopdf::ObjectId)> = doc.get_pages().into_iter().collect();
    pages.sort_by_key(|(n, _)| *n);
    if pages.is_empty() {
        return Err(ImposerError::InvalidPageCount(0));
    }
    let mut out = Vec::new();
    for (idx, (_, pid)) in pages.into_iter().enumerate() {
        let (w, h) = crate::pdf::get_page_dimensions(&doc, pid)?;
        out.push(SourcePage {
            width_pt: w,
            height_pt: h,
            kind: SourceKind::PdfPage {
                bytes: bytes.to_vec(),
                page_index: idx,
            },
        });
    }
    Ok(out)
}

// ---------- PNG ----------
fn parse_png(bytes: &[u8]) -> Result<SourcePage> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .map_err(|e| ImposerError::Parse(format!("PNG : {e}")))?;
    let rgb = img.to_rgb8();
    let (dpi_x, dpi_y) = png_dpi(bytes).unwrap_or((300.0, 300.0));
    let w_px = rgb.width();
    let h_px = rgb.height();
    let width_pt = w_px as f32 / dpi_x * 72.0;
    let height_pt = h_px as f32 / dpi_y * 72.0;
    Ok(SourcePage {
        width_pt,
        height_pt,
        kind: SourceKind::Raster {
            pixels: rgb.into_raw(),
            width_px: w_px,
            height_px: h_px,
            color_space: ImageColorSpace::DeviceRgb,
            icc_profile: None,
        },
    })
}

fn png_dpi(bytes: &[u8]) -> Option<(f32, f32)> {
    let mut i = 8usize;
    while i + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[i..i + 4].try_into().ok()?) as usize;
        if i + 8 + len + 4 > bytes.len() {
            break;
        }
        let typ = &bytes[i + 4..i + 8];
        let data_start = i + 8;
        if typ == b"pHYs" && len == 9 {
            let px = u32::from_be_bytes(bytes[data_start..data_start + 4].try_into().ok()?);
            let py = u32::from_be_bytes(bytes[data_start + 4..data_start + 8].try_into().ok()?);
            let unit = bytes[data_start + 8];
            if unit == 1 {
                let dpi_x = px as f32 / 39.370_08;
                let dpi_y = py as f32 / 39.370_08;
                return Some((dpi_x, dpi_y));
            }
        }
        i = data_start + len + 4; // skip data + crc
    }
    None
}

// ---------- JPEG ----------
fn parse_jpeg(bytes: &[u8]) -> Result<SourcePage> {
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
        .map_err(|e| ImposerError::Parse(format!("JPEG : {e}")))?;
    let rgb = img.to_rgb8();
    let (dpi_x, dpi_y) = jpeg_dpi(bytes).unwrap_or((300.0, 300.0));
    let w_px = rgb.width();
    let h_px = rgb.height();
    let width_pt = w_px as f32 / dpi_x * 72.0;
    let height_pt = h_px as f32 / dpi_y * 72.0;
    Ok(SourcePage {
        width_pt,
        height_pt,
        kind: SourceKind::Raster {
            pixels: rgb.into_raw(),
            width_px: w_px,
            height_px: h_px,
            color_space: ImageColorSpace::DeviceRgb,
            icc_profile: None,
        },
    })
}

fn jpeg_dpi(bytes: &[u8]) -> Option<(f32, f32)> {
    let mut i = 2usize; // after SOI
    let n = bytes.len();
    while i + 1 < n {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        // skip fill bytes
        let marker = bytes[i + 1];
        if marker == 0x00 {
            i += 2;
            continue;
        }
        if marker == 0xD9 || marker == 0xDA {
            break; // EOI or SOS
        }
        // standalone markers without length
        if (0xD0..=0xD9).contains(&marker) && marker != 0xD8 {
            i += 2;
            continue;
        }
        if i + 4 > n {
            break;
        }
        let seglen = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if seglen < 2 || i + 2 + seglen > n {
            break;
        }
        if marker == 0xE0 && seglen >= 14 && &bytes[i + 4..i + 8] == b"JFIF\0" {
            let units = bytes[i + 11];
            let x = u16::from_be_bytes([bytes[i + 12], bytes[i + 13]]);
            let y = u16::from_be_bytes([bytes[i + 14], bytes[i + 15]]);
            if units == 1 {
                return Some((x as f32, y as f32));
            }
            if units == 2 {
                return Some((x as f32 * 2.54, y as f32 * 2.54));
            }
        }
        i += 2 + seglen;
    }
    None
}

// ---------- PSD ----------
fn parse_psd(bytes: &[u8]) -> Result<SourcePage> {
    // Le décodeur interne lit directement l'Image Data PSD, préservant le
    // comportement composite observé sans appeler ImageMagick.
    match crate::psd_composite::parse_psd_composite(bytes) {
        Ok(source) => return Ok(source),
        Err(e @ ImposerError::NoProfile)
        | Err(e @ ImposerError::InvalidIcc(_))
        | Err(e @ ImposerError::AmbiguousProfile) => return Err(e),
        Err(_) => {}
    }

    let cmyk_mode = psd_color_mode(bytes) == Some(4);
    match parse_psd_zune(bytes) {
        Ok(s) => Ok(s),
        Err(e @ ImposerError::NoProfile)
        | Err(e @ ImposerError::InvalidIcc(_))
        | Err(e @ ImposerError::AmbiguousProfile) => Err(e),
        // CMYK PSDs must not be silently downgraded to an RGB approximation.
        Err(e) if cmyk_mode => Err(e),
        Err(_) => parse_psd_legacy(bytes),
    }
}

fn psd_color_mode(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 26 || !bytes.starts_with(b"8BPS") {
        return None;
    }
    // PSD header: signature(4) version(2) reserved(6) channels(2)
    // height(4) width(4) depth(2) color_mode(2) => color_mode at offset 24.
    Some(u16::from_be_bytes(bytes[24..26].try_into().ok()?))
}

fn parse_psd_zune(bytes: &[u8]) -> Result<SourcePage> {
    use zune_psd::PSDDecoder;
    use zune_psd::zune_core::bytestream::ZCursor;

    let (dpi_x, dpi_y) = extract_psd_resolution(bytes).unwrap_or((300.0, 300.0));

    let mut decoder = PSDDecoder::new(ZCursor::new(bytes));
    decoder
        .decode_headers()
        .map_err(|e| ImposerError::Parse(format!("PSD : {e:?}")))?;
    let (w, h) = decoder
        .dimensions()
        .ok_or_else(|| ImposerError::Parse("PSD : dimensions manquantes".to_string()))?;
    let colorspace = decoder
        .colorspace()
        .ok_or_else(|| ImposerError::Parse("PSD : espace couleur non supporté".to_string()))?;

    let raw = decoder
        .decode_raw()
        .map_err(|e| ImposerError::Parse(format!("PSD : {e:?}")))?;
    let raw = if decoder.bit_depth() == Some(zune_psd::zune_core::bit_depth::BitDepth::Sixteen) {
        // Downconvert 16-bit to 8-bit without treating planar data as RGBA.
        // Each plane is laid out contiguously and header channel counts apply.
        raw.as_chunks::<2>().0.iter().map(|b| b[0]).collect()
    } else {
        raw
    };

    if w == 0 || h == 0 {
        return Err(ImposerError::Parse(
            "PSD : dimensions invalides".to_string(),
        ));
    }
    let pixels = w * h;
    if pixels == 0 || raw.len() < pixels {
        return Err(ImposerError::Parse("PSD : données incomplètes".to_string()));
    }
    let channels = raw.len() / pixels;
    let rgb = match colorspace {
        zune_psd::zune_core::colorspace::ColorSpace::RGB
        | zune_psd::zune_core::colorspace::ColorSpace::RGBA => {
            if channels == 3 {
                deinterleave_rgb(&raw, pixels)
            } else if channels == 4 {
                deinterleave_rgba_to_rgb(&raw, pixels)
            } else {
                return Err(ImposerError::Parse("PSD : canal inattendu".to_string()));
            }
        }
        zune_psd::zune_core::colorspace::ColorSpace::Luma => {
            deinterleave_gray(&raw, pixels, channels)
        }
        zune_psd::zune_core::colorspace::ColorSpace::LumaA => deinterleave_gray_alpha(&raw, pixels),
        zune_psd::zune_core::colorspace::ColorSpace::CMYK => {
            if channels != 4 {
                return Err(ImposerError::Parse(
                    "PSD : nombre de canaux CMJN inattendu".to_string(),
                ));
            }

            let inverted: Vec<u8> = raw.iter().map(|v| 255u8.wrapping_sub(*v)).collect();
            let icc_bytes = extract_psd_embedded_profile(bytes).ok_or(ImposerError::NoProfile)?;
            IccProfile::parse(&icc_bytes).map_err(|e| ImposerError::InvalidIcc(e.to_string()))?;
            let width_pt = w as f32 / dpi_x * 72.0;
            let height_pt = h as f32 / dpi_y * 72.0;
            return Ok(SourcePage {
                width_pt,
                height_pt,
                kind: SourceKind::Raster {
                    pixels: inverted,
                    width_px: w as u32,
                    height_px: h as u32,
                    color_space: ImageColorSpace::DeviceCmyk,
                    icc_profile: Some(icc_bytes),
                },
            });
        }
        _ => {
            return Err(ImposerError::Parse(
                "PSD : espace couleur non supporté".to_string(),
            ));
        }
    };

    Ok(make_raster_source(
        w as u32,
        h as u32,
        rgb,
        dpi_x,
        dpi_y,
        ImageColorSpace::DeviceRgb,
    ))
}

fn deinterleave_rgb(raw: &[u8], pixels: usize) -> Vec<u8> {
    let channels = raw.len() / pixels;
    let mut out = vec![0u8; pixels * 3];
    for p in 0..pixels {
        out[p * 3] = raw[p * channels];
        out[p * 3 + 1] = raw[p * channels + 1];
        out[p * 3 + 2] = raw[p * channels + 2];
    }
    out
}

fn deinterleave_rgba_to_rgb(raw: &[u8], pixels: usize) -> Vec<u8> {
    let channels = raw.len() / pixels;
    let mut out = vec![0u8; pixels * 3];
    for p in 0..pixels {
        let r = raw[p * channels];
        let g = raw[p * channels + 1];
        let b = raw[p * channels + 2];
        let a = raw[p * channels + 3];
        let af = a as f32 / 255.0;
        out[p * 3] = (r as f32 * af + 255.0 * (1.0 - af)).round() as u8;
        out[p * 3 + 1] = (g as f32 * af + 255.0 * (1.0 - af)).round() as u8;
        out[p * 3 + 2] = (b as f32 * af + 255.0 * (1.0 - af)).round() as u8;
    }
    out
}

fn deinterleave_gray(raw: &[u8], pixels: usize, channels: usize) -> Vec<u8> {
    let channels = channels.max(1);
    let mut out = vec![0u8; pixels * 3];
    for p in 0..pixels {
        let v = raw[p * channels];
        out[p * 3] = v;
        out[p * 3 + 1] = v;
        out[p * 3 + 2] = v;
    }
    out
}

fn deinterleave_gray_alpha(raw: &[u8], pixels: usize) -> Vec<u8> {
    let channels = (raw.len() / pixels).max(2);
    let mut out = vec![0u8; pixels * 3];
    for p in 0..pixels {
        let v = raw[p * channels];
        let a = raw[p * channels + 1];
        let af = a as f32 / 255.0;
        let vv = (v as f32 * af + 255.0 * (1.0 - af)).round() as u8;
        out[p * 3] = vv;
        out[p * 3 + 1] = vv;
        out[p * 3 + 2] = vv;
    }
    out
}

fn parse_psd_legacy(bytes: &[u8]) -> Result<SourcePage> {
    let psd = Psd::from_bytes(bytes).map_err(|e| ImposerError::Parse(format!("PSD : {e}")))?;
    let w_px = psd.width();
    let h_px = psd.height();
    let rgba = psd.rgba();
    if rgba.len() != (w_px * h_px * 4) as usize {
        // Try flatten_layers RGBA
        let filtered = psd.flatten_layers_rgba(&|_| true);
        match filtered {
            Ok(v) if v.len() == (w_px * h_px * 4) as usize => {
                let rgb = rgba_to_rgb(&v);
                return Ok(make_raster_source(
                    w_px,
                    h_px,
                    rgb,
                    300.0,
                    300.0,
                    ImageColorSpace::DeviceRgb,
                ));
            }
            _ => {}
        }
        return Err(ImposerError::Parse(
            "PSD : données RGBA incohérentes".to_string(),
        ));
    }
    let rgb = rgba_to_rgb(&rgba);
    Ok(make_raster_source(
        w_px,
        h_px,
        rgb,
        300.0,
        300.0,
        ImageColorSpace::DeviceRgb,
    ))
}

fn rgba_to_rgb(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() / 4 * 3);
    let mut i = 0;
    while i + 3 < rgba.len() {
        let r = rgba[i];
        let g = rgba[i + 1];
        let b = rgba[i + 2];
        let a = rgba[i + 3];
        // Compose alpha over white
        let a = a as f32 / 255.0;
        let rr = (r as f32 * a + 255.0 * (1.0 - a)).round() as u8;
        let gg = (g as f32 * a + 255.0 * (1.0 - a)).round() as u8;
        let bb = (b as f32 * a + 255.0 * (1.0 - a)).round() as u8;
        out.push(rr);
        out.push(gg);
        out.push(bb);
        i += 4;
    }
    out
}

fn make_raster_source(
    w_px: u32,
    h_px: u32,
    rgb: Vec<u8>,
    dpi_x: f32,
    dpi_y: f32,
    color_space: ImageColorSpace,
) -> SourcePage {
    let width_pt = w_px as f32 / dpi_x * 72.0;
    let height_pt = h_px as f32 / dpi_y * 72.0;
    SourcePage {
        width_pt,
        height_pt,
        kind: SourceKind::Raster {
            pixels: rgb,
            width_px: w_px,
            height_px: h_px,
            color_space,
            icc_profile: None,
        },
    }
}

pub(crate) fn extract_psd_resolution(bytes: &[u8]) -> Option<(f32, f32)> {
    let (cmd_len, header_end, resources_off, reslen, resources_end) =
        psd_resource_block_bounds(bytes)?;
    let mut pos = resources_off;
    while pos + 10 <= resources_end {
        if &bytes[pos..pos + 4] != b"8BIM" {
            break;
        }
        let rid = u16::from_be_bytes(bytes[pos + 4..pos + 6].try_into().ok()?) as u32;
        let name_len = bytes[pos + 6] as usize;
        let mut field = 1usize + name_len;
        if field % 2 == 1 {
            field += 1;
        }
        let size_off = pos + 6 + field;
        if size_off + 4 > resources_end {
            break;
        }
        let size = u32::from_be_bytes(bytes[size_off..size_off + 4].try_into().ok()?) as usize;
        let data_off = size_off + 4;
        if data_off + size > resources_end {
            break;
        }
        if rid == 0x03ED && size >= 16 {
            let data = &bytes[data_off..data_off + size];
            let h_fixed = u32::from_be_bytes(data[0..4].try_into().ok()?);
            let h_unit = u16::from_be_bytes(data[4..6].try_into().ok()?);
            let v_fixed = u32::from_be_bytes(data[8..12].try_into().ok()?);
            let v_unit = u16::from_be_bytes(data[12..14].try_into().ok()?);
            let dpi_x = fixed_psd_resolution(h_fixed, h_unit);
            let dpi_y = fixed_psd_resolution(v_fixed, v_unit);
            if dpi_x > 0.0 && dpi_y > 0.0 {
                return Some((dpi_x, dpi_y));
            }
        }
        pos = data_off + size;
        if pos % 2 == 1 {
            pos += 1;
        }
    }
    // keep compiler happy about unused bounds variables
    let _ = (cmd_len, header_end, resources_off, reslen, resources_end);
    None
}

fn fixed_psd_resolution(fixed: u32, unit: u16) -> f32 {
    let value = fixed as f32 / 65536.0;
    match unit {
        2 => value * 2.54, // pixels per cm -> dpi
        _ => value,        // pixels per inch (or unknown -> assume inch)
    }
}

fn psd_resource_block_bounds(bytes: &[u8]) -> Option<(usize, usize, usize, usize, usize)> {
    if bytes.len() < 34 {
        return None;
    }
    let cmd_len = u32::from_be_bytes(bytes[26..30].try_into().ok()?) as usize;
    let header_end = 30usize.checked_add(cmd_len)?;
    let resources_off = header_end.checked_add(4)?;
    if resources_off > bytes.len() {
        return None;
    }
    let reslen = u32::from_be_bytes(bytes[header_end..resources_off].try_into().ok()?) as usize;
    let resources_end = resources_off.checked_add(reslen)?;
    if resources_end > bytes.len() {
        return None;
    }
    Some((cmd_len, header_end, resources_off, reslen, resources_end))
}

pub(crate) fn extract_psd_embedded_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 34 {
        return None;
    }
    let cmd_len = u32::from_be_bytes(bytes[26..30].try_into().ok()?) as usize;
    let header_end = 30usize.checked_add(cmd_len)?;
    let resources_off = header_end.checked_add(4)?;
    if resources_off > bytes.len() {
        return None;
    }
    let reslen = u32::from_be_bytes(bytes[header_end..resources_off].try_into().ok()?) as usize;
    let resources_end = resources_off.checked_add(reslen)?;
    if resources_end > bytes.len() {
        return None;
    }
    let mut pos = resources_off;
    while pos + 10 <= resources_end {
        if &bytes[pos..pos + 4] != b"8BIM" {
            break;
        }
        let rid = u16::from_be_bytes(bytes[pos + 4..pos + 6].try_into().ok()?) as u32;
        let name_len = bytes[pos + 6] as usize;
        let mut field = 1usize + name_len;
        if field % 2 == 1 {
            field += 1;
        }
        let size_off = pos + 6 + field;
        if size_off + 4 > resources_end {
            break;
        }
        let size = u32::from_be_bytes(bytes[size_off..size_off + 4].try_into().ok()?) as usize;
        let data_off = size_off + 4;
        if data_off + size > resources_end {
            break;
        }
        if rid == 0x040f {
            return Some(bytes[data_off..data_off + size].to_vec());
        }
        pos = data_off + size;
        if pos % 2 == 1 {
            pos += 1;
        }
    }
    None
}

fn pdf_color_kind(bytes: &[u8], page_index: usize) -> SourceColorKind {
    if let Ok(src) = open_pdf_source(bytes.to_vec())
        && let Ok(det) = crate::profile::detect_cmyk_profile(&src, page_index, None)
        && det.profile.is_some()
    {
        return SourceColorKind::Cmyk;
    }
    if bytes.windows(10).any(|w| w == b"DeviceCMYK") {
        return SourceColorKind::Cmyk;
    }
    SourceColorKind::Rgb
}

fn pdf_color_label(bytes: &[u8], page_index: usize) -> String {
    if let Ok(src) = open_pdf_source(bytes.to_vec())
        && let Ok(det) = crate::profile::detect_cmyk_profile(&src, page_index, None)
        && let Some(p) = det.profile
    {
        if let Some(desc) = p.info.description.as_ref().filter(|s| !s.is_empty()) {
            return format!("CMJN ({desc})");
        }
        return "CMJN (profil ICC détecté)".to_string();
    }
    if bytes.windows(10).any(|w| w == b"DeviceCMYK") {
        "CMJN (sans profil identifiable)".to_string()
    } else {
        "RVB (pas de conversion)".to_string()
    }
}

/// Source PDF ouvert (lecture + détection), pour profile/import.
#[derive(Debug)]
pub struct SourcePdf {
    pub bytes: Vec<u8>,
    pub document: lopdf::Document,
}

pub fn open_pdf_source(bytes: Vec<u8>) -> crate::error::Result<SourcePdf> {
    let doc = lopdf::Document::load_mem(&bytes)
        .map_err(|e| crate::error::ImposerError::Parse(e.to_string()))?;
    if doc.is_encrypted() {
        return Err(crate::error::ImposerError::EncryptedPdf);
    }
    if doc.get_pages().is_empty() {
        return Err(crate::error::ImposerError::InvalidPageCount(0));
    }
    Ok(SourcePdf {
        bytes,
        document: doc,
    })
}

pub fn page_object_ids(doc: &lopdf::Document) -> Vec<lopdf::ObjectId> {
    let mut v: Vec<(u32, lopdf::ObjectId)> = doc.get_pages().into_iter().collect();
    v.sort_by_key(|(n, _)| *n);
    v.into_iter().map(|(_, id)| id).collect()
}

pub fn inherited_page_dict(
    doc: &lopdf::Document,
    page_id: lopdf::ObjectId,
) -> crate::error::Result<lopdf::Dictionary> {
    use lopdf::{Dictionary, Object};
    let page_obj = doc
        .get_object(page_id)
        .map_err(|e| crate::error::ImposerError::Parse(e.to_string()))?;
    let mut page = page_obj
        .as_dict()
        .map_err(|e| crate::error::ImposerError::Parse(e.to_string()))?
        .clone();

    let mut parents: Vec<lopdf::ObjectId> = Vec::new();
    let mut cur = page_id;
    while let Ok(obj) = doc.get_object(cur) {
        let dict = match obj.as_dict() {
            Ok(d) => d,
            Err(_) => break,
        };
        match dict.get(b"Parent") {
            Ok(Object::Reference(pid)) => {
                cur = *pid;
                parents.push(cur);
            }
            _ => break,
        }
    }

    const INHERITED: &[&[u8]] = &[
        b"Resources",
        b"MediaBox",
        b"CropBox",
        b"BleedBox",
        b"TrimBox",
        b"ArtBox",
        b"Rotate",
        b"UserUnit",
    ];
    for key in INHERITED {
        if page.get(key).is_err() {
            for anc in &parents {
                if let Ok(Object::Dictionary(anc_dict)) = doc.get_object(*anc)
                    && let Ok(v) = anc_dict.get(key)
                {
                    page.set(key.to_vec(), v.clone());
                    break;
                }
            }
        }
    }
    if page.get(b"Resources").is_err() {
        page.set(b"Resources", Object::Dictionary(Dictionary::new()));
    }
    Ok(page)
}

pub fn decompress_stream(stream: &lopdf::Stream) -> crate::error::Result<Vec<u8>> {
    stream
        .decompressed_content()
        .map_err(|e| crate::error::ImposerError::UnsupportedFilter(e.to_string()))
}

pub(crate) fn make_psd_source_page(
    width_px: u32,
    height_px: u32,
    pixels: Vec<u8>,
    color_space: ImageColorSpace,
    dpi: (f32, f32),
    icc_profile: Option<Vec<u8>>,
) -> Result<SourcePage> {
    let expected = width_px as usize
        * height_px as usize
        * match color_space {
            ImageColorSpace::DeviceRgb | ImageColorSpace::DeviceGray => 3,
            ImageColorSpace::DeviceCmyk => 4,
        };
    if pixels.len() != expected {
        return Err(ImposerError::Parse(format!(
            "taille pixels PSD source inattendue : {} != {}",
            pixels.len(),
            expected
        )));
    }
    let width_pt = width_px as f32 / dpi.0 * 72.0;
    let height_pt = height_px as f32 / dpi.1 * 72.0;
    Ok(SourcePage {
        width_pt,
        height_pt,
        kind: SourceKind::Raster {
            pixels,
            width_px,
            height_px,
            color_space,
            icc_profile,
        },
    })
}
