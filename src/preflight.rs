//! Contrôles des sources et de la sortie (preflight).

use crate::source::SourcePdf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorPolicy {
    Strict,
    Lenient,
}

#[derive(Debug, Clone, Default)]
pub struct PreflightReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl PreflightReport {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

pub fn preflight(src: &SourcePdf) -> PreflightReport {
    preflight_with(src, ColorPolicy::Strict)
}

pub fn preflight_with(src: &SourcePdf, policy: ColorPolicy) -> PreflightReport {
    let mut report = PreflightReport::default();
    let doc = &src.document;
    // encryption
    if doc.is_encrypted() {
        report.errors.push("PDF chiffré".to_string());
    }
    // scan objects
    let mut has_js = false;
    let mut has_ocg = false;
    let mut has_transparency = false;
    let mut has_annotations = false;
    for obj in doc.objects.values() {
        // Color checks via stream dicts / names
        if let lopdf::Object::Stream(s) = obj {
            // no direct checks here; resource scanning below
            let _ = s;
        }
        if let lopdf::Object::Dictionary(d) = obj {
            // annotations
            if d.get(b"Type").ok().and_then(as_name_str) == Some("Annot".to_string()) {
                has_annotations = true;
            }
            // OCG
            if d.get(b"OCProperties").is_ok() {
                has_ocg = true;
            }
            // transparency
            if let Ok(s) = d.get(b"S")
                && as_name_str(s) == Some("Transparency".to_string())
            {
                has_transparency = true;
            }
            // javascript actions
            if d.get(b"JavaScript").is_ok() || d.get(b"JS").is_ok() {
                has_js = true;
            }
            // font embedding
            if d.get(b"Type").ok().and_then(as_name_str) == Some("Font".to_string()) {
                let subtype = d
                    .get(b"Subtype")
                    .ok()
                    .and_then(as_name_str)
                    .unwrap_or_default();
                if subtype != "Type3" {
                    let embedded = d.get(b"FontFile").is_ok()
                        || d.get(b"FontFile2").is_ok()
                        || d.get(b"FontFile3").is_ok();
                    if !embedded {
                        let msg = "police non incorporée".to_string();
                        if policy == ColorPolicy::Strict {
                            report.errors.push(msg);
                        } else {
                            report.warnings.push(msg);
                        }
                    }
                }
            }
        }
    }
    if has_js {
        report.errors.push("JavaScript détecté".to_string());
    }
    if has_ocg {
        report
            .warnings
            .push("couches optionnelles (OCG) détectées".to_string());
    }
    if has_transparency {
        report.warnings.push("transparence détectée".to_string());
    }
    if has_annotations {
        report.warnings.push("annotations détectées".to_string());
    }

    // Color space scan: recursively collect resource dicts
    let ids = crate::source::page_object_ids(doc);
    for pid in ids {
        if let Ok(page) = crate::source::inherited_page_dict(doc, pid) {
            collect_color_issues(doc, &page, policy, &mut report);
        }
    }
    // macro to bump? Already.

    report
}

fn collect_color_issues(
    doc: &lopdf::Document,
    res_owner: &lopdf::Dictionary,
    policy: ColorPolicy,
    report: &mut PreflightReport,
) {
    // traverse resource dict if present
    let resources = match res_owner.get(b"Resources") {
        Ok(lopdf::Object::Dictionary(d)) => Some(d.clone()),
        Ok(lopdf::Object::Reference(id)) => {
            if let Ok(lopdf::Object::Dictionary(d)) = doc.get_object(*id) {
                Some(d.clone())
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(res) = resources {
        scan_resources_colors(
            doc,
            &res,
            policy,
            report,
            &mut std::collections::HashSet::new(),
        );
    }
}

#[allow(clippy::only_used_in_recursion)]
fn scan_resources_colors(
    doc: &lopdf::Document,
    res: &lopdf::Dictionary,
    policy: ColorPolicy,
    report: &mut PreflightReport,
    visited: &mut std::collections::HashSet<lopdf::ObjectId>,
) {
    // ColorSpace entries
    if let Ok(o) = res.get(b"ColorSpace") {
        let dict = resolve_dict(doc, o);
        if let Some(d) = dict {
            for (_, v) in d {
                color_issue_for_value(doc, &v, policy, report, visited);
            }
        }
    }
    // XObject
    if let Ok(o) = res.get(b"XObject") {
        let dict = resolve_dict(doc, o);
        if let Some(d) = dict {
            for (_, v) in d {
                if let lopdf::Object::Reference(id) = v
                    && visited.insert(id)
                    && let Ok(lopdf::Object::Stream(s)) = doc.get_object(id)
                {
                    if let Ok(cs) = s.dict.get(b"ColorSpace") {
                        color_issue_for_value(doc, cs, policy, report, visited);
                    }
                    if let Ok(r) = s.dict.get(b"Resources") {
                        let rd = resolve_dict(doc, r);
                        if let Some(d2) = rd {
                            scan_resources_colors(doc, &d2, policy, report, visited);
                        }
                    }
                }
            }
        }
    }
    // Shading
    if let Ok(o) = res.get(b"Shading") {
        let dict = resolve_dict(doc, o);
        if let Some(d) = dict {
            for (_, v) in d {
                if let lopdf::Object::Reference(id) = v {
                    if let Ok(lopdf::Object::Dictionary(sd)) = doc.get_object(id)
                        && let Ok(cs) = sd.get(b"ColorSpace")
                    {
                        color_issue_for_value(doc, cs, policy, report, visited);
                    }
                } else if let lopdf::Object::Dictionary(sd) = v
                    && let Ok(cs) = sd.get(b"ColorSpace")
                {
                    color_issue_for_value(doc, cs, policy, report, visited);
                }
            }
        }
    }
}

#[allow(clippy::only_used_in_recursion)]
fn color_issue_for_value(
    doc: &lopdf::Document,
    obj: &lopdf::Object,
    policy: ColorPolicy,
    report: &mut PreflightReport,
    visited: &mut std::collections::HashSet<lopdf::ObjectId>,
) {
    let target = match obj {
        lopdf::Object::Reference(id) => {
            if let Ok(o) = doc.get_object(*id) {
                std::borrow::Cow::Owned(o.clone())
            } else {
                return;
            }
        }
        other => std::borrow::Cow::Borrowed(other),
    };
    match &*target {
        lopdf::Object::Name(n) => {
            let name = String::from_utf8_lossy(n);
            push_color_name(&name, policy, report);
        }
        lopdf::Object::Array(arr) => {
            if let Some(lopdf::Object::Name(n)) = arr.first() {
                let name = String::from_utf8_lossy(n);
                if name == "ICCBased" {
                    // get stream
                    if let Some(lopdf::Object::Reference(sid)) = arr.get(1)
                        && let Ok(lopdf::Object::Stream(s)) = doc.get_object(*sid)
                        && let Ok(data) = crate::source::decompress_stream(s)
                    {
                        // check profile color space CMYK
                        if data.len() >= 20 {
                            let cs = &data[16..20];
                            if cs != b"CMYK" {
                                report
                                    .errors
                                    .push("profil ICC non CMJN détecté".to_string());
                            }
                        }
                    }
                } else {
                    push_color_name(&name, policy, report);
                }
            }
            for e in arr {
                // nested
                color_issue_for_value(doc, e, policy, report, visited);
            }
        }
        _ => {}
    }
}

fn push_color_name(name: &str, policy: ColorPolicy, report: &mut PreflightReport) {
    match name {
        "DeviceRGB" => {
            let m = "DeviceRGB détecté".to_string();
            if policy == ColorPolicy::Strict {
                report.errors.push(m);
            } else {
                report.warnings.push(m);
            }
        }
        "DeviceGray" => report.warnings.push("DeviceGray détecté".to_string()),
        "Lab" | "CalRGB" => {
            let m = format!("{name} détecté");
            report.errors.push(m);
        }
        _ => {}
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

fn as_name_str(o: &lopdf::Object) -> Option<String> {
    if let lopdf::Object::Name(n) = o {
        Some(String::from_utf8_lossy(n).to_string())
    } else {
        None
    }
}

pub fn preflight_output(data: &[u8]) -> PreflightReport {
    let mut report = PreflightReport::default();
    // header version
    if !data.starts_with(b"%PDF-") {
        report.errors.push("en-tête PDF manquant".to_string());
        return report;
    }
    let header = String::from_utf8_lossy(&data[..8.min(data.len())]);
    let version = header.trim_start_matches("%PDF-");
    let vnum: f32 = version.parse().unwrap_or(0.0);
    if vnum < 1.6 {
        report.errors.push(format!("version PDF < 1.6 : {version}"));
    }

    let doc = match lopdf::Document::load_mem(data) {
        Ok(d) => d,
        Err(e) => {
            report.errors.push(format!("lecture sortie : {e}"));
            return report;
        }
    };

    if doc.is_encrypted() {
        report.errors.push("sortie chiffrée".to_string());
    }

    // catalog OutputIntents
    if let Ok(cat_id) = doc.trailer.get(b"Root").and_then(|o| {
        if let lopdf::Object::Reference(id) = o {
            Ok(*id)
        } else {
            Err(lopdf::Error::ObjectNotFound((0, 0)))
        }
    }) && let Ok(lopdf::Object::Dictionary(cat)) = doc.get_object(cat_id)
    {
        if cat.get(b"OutputIntents").is_err() {
            report.errors.push("OutputIntents manquant".to_string());
        } else if let Ok(lopdf::Object::Array(arr)) = cat.get(b"OutputIntents")
            && let Some(lopdf::Object::Reference(iid)) = arr.first()
            && let Ok(lopdf::Object::Dictionary(intent)) = doc.get_object(*iid)
        {
            let s = intent
                .get(b"S")
                .ok()
                .and_then(as_name_str)
                .unwrap_or_default();
            if s != "GTS_PDFX" {
                report.errors.push("OutputIntent S != GTS_PDFX".to_string());
            }
            if let Ok(lopdf::Object::Reference(pid)) = intent.get(b"DestOutputProfile") {
                if let Ok(lopdf::Object::Stream(s)) = doc.get_object(*pid) {
                    let n = s
                        .dict
                        .get(b"N")
                        .ok()
                        .and_then(|o| o.as_i64().ok())
                        .unwrap_or(0);
                    if n != 4 {
                        report
                            .errors
                            .push(format!("DestOutputProfile N={n} (attendu 4)"));
                    }
                }
            } else {
                report.errors.push("DestOutputProfile manquant".to_string());
            }
        }
        // Metadata
        if let Ok(lopdf::Object::Reference(mid)) = cat.get(b"Metadata") {
            if let Ok(lopdf::Object::Stream(s)) = doc.get_object(*mid) {
                let txt = String::from_utf8_lossy(&s.content);
                if !txt.contains("GTS_PDFXVersion") || !txt.contains("PDF/X-4") {
                    report
                        .errors
                        .push("XMP GTS_PDFXVersion/PDF/X-4 manquant".to_string());
                }
            }
        } else {
            report.warnings.push("Metadata XMP manquante".to_string());
        }
    }

    // Info dict
    if let Ok(lopdf::Object::Reference(iid)) = doc.trailer.get(b"Info") {
        if let Ok(lopdf::Object::Dictionary(info)) = doc.get_object(*iid) {
            for key in [
                b"Title" as &[u8],
                b"CreationDate",
                b"ModDate",
                b"Producer",
                b"GTS_PDFXVersion",
                b"Trapped",
            ] {
                if info.get(key).is_err() {
                    report
                        .errors
                        .push(format!("Info/{} manquant", String::from_utf8_lossy(key)));
                }
            }
            if let Ok(v) = info.get(b"GTS_PDFXVersion")
                && (if let lopdf::Object::String(b, _) = v {
                    String::from_utf8_lossy(b).to_string()
                } else {
                    String::new()
                }) != "PDF/X-4"
            {
                report.errors.push("GTS_PDFXVersion != PDF/X-4".to_string());
            }
        }
    } else {
        report.errors.push("Info manquant".to_string());
    }

    // /ID
    if let Ok(lopdf::Object::Array(arr)) = doc.trailer.get(b"ID") {
        if arr.len() < 2 {
            report.errors.push("trailer ID incomplet".to_string());
        } else {
            let ok = arr.iter().all(|o| {
                if let lopdf::Object::String(b, _) = o {
                    b.len() == 16
                } else {
                    false
                }
            });
            if !ok {
                report
                    .errors
                    .push("trailer ID chaînes de 16 octets requises".to_string());
            }
        }
    } else {
        report.errors.push("trailer ID manquant".to_string());
    }

    // Pages: TrimBox
    let pages = doc.get_pages();
    for pid in pages.values() {
        if let Ok(lopdf::Object::Dictionary(p)) = doc.get_object(*pid)
            && p.get(b"TrimBox").is_err()
        {
            report
                .errors
                .push("TrimBox manquant sur une page".to_string());
        }
        // annotations?
    }
    // boxes inclusion check on first page
    if let Some((_, pid)) = pages.iter().next()
        && let Ok(lopdf::Object::Dictionary(p)) = doc.get_object(*pid)
        && let (Ok(m), Ok(b), Ok(t)) = (p.get(b"MediaBox"), p.get(b"BleedBox"), p.get(b"TrimBox"))
        && let (Some(m), Some(b), Some(t)) = (as_array_f64(m), as_array_f64(b), as_array_f64(t))
    {
        // media >= bleed >= trim (compare widths/heights)
        if !(m[2] - m[0] >= b[2] - b[0] && m[3] - m[1] >= b[3] - b[1]) {
            report
                .errors
                .push("MediaBox ne contient pas BleedBox".to_string());
        }
        if !(b[2] - b[0] >= t[2] - t[0] && b[3] - b[1] >= t[3] - t[1]) {
            report
                .errors
                .push("BleedBox ne contient pas TrimBox".to_string());
        }
    }

    // no annotations check simple
    for obj in doc.objects.values() {
        if let lopdf::Object::Dictionary(d) = obj
            && d.get(b"Type").ok().and_then(as_name_str) == Some("Annot".to_string())
        {
            report
                .errors
                .push("annotation détectée dans la sortie".to_string());
        }
    }

    report
}

fn as_array_f64(o: &lopdf::Object) -> Option<[f64; 4]> {
    if let lopdf::Object::Array(a) = o
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
