use imposer::config::{DocumentOrientation, SheetOrientation, SheetSize};
use imposer::source::SourceColorKind;
use imposer::{ImpositionConfig, impose, impose_sources, sources_from_bytes};
use lopdf::Document;
use std::fs;

fn load(name: &str) -> Vec<u8> {
    fs::read(format!("tests/fixtures/{}", name)).unwrap()
}

#[test]
fn one_page_output() {
    let input = load("sample_1page.pdf");
    let cfg = ImpositionConfig {
        sheet_size: SheetSize::A4,
        sheet_orientation: SheetOrientation::Auto,
        bleed_mm: 2.0,
        safety_margin_mm: 5.0,
        document_orientation: DocumentOrientation::Portrait,
        rotate_copies: true,
    };
    let bytes = impose(&input, &cfg).unwrap();
    let doc = Document::load_mem(&bytes).unwrap();
    assert_eq!(doc.get_pages().len(), 1);

    let pages: Vec<_> = doc.get_pages().values().copied().collect();
    let page = doc.get_object(pages[0]).unwrap().as_dict().unwrap();
    let mb = page.get(b"MediaBox").unwrap().as_array().unwrap();
    let w = mb[2].as_float().unwrap();
    let h = mb[3].as_float().unwrap();
    assert!((w - 595.276).abs() < 1.0, "w={w}");
    assert!((h - 841.89).abs() < 1.0, "h={h}");
}

#[test]
fn two_page_output() {
    let input = load("sample_2page.pdf");
    let cfg = ImpositionConfig {
        sheet_size: SheetSize::Sra3,
        sheet_orientation: SheetOrientation::Auto,
        bleed_mm: 2.0,
        safety_margin_mm: 5.0,
        document_orientation: DocumentOrientation::Portrait,
        rotate_copies: true,
    };
    let bytes = impose(&input, &cfg).unwrap();
    let doc = Document::load_mem(&bytes).unwrap();
    assert_eq!(doc.get_pages().len(), 2);
}

#[test]
fn output_intent_propagated() {
    let input = load("sample_1page.pdf");
    let cfg = ImpositionConfig {
        sheet_size: SheetSize::A4,
        sheet_orientation: SheetOrientation::Auto,
        bleed_mm: 2.0,
        safety_margin_mm: 5.0,
        document_orientation: DocumentOrientation::Portrait,
        rotate_copies: true,
    };
    let src = Document::load_mem(&input).unwrap();
    let bytes = impose(&input, &cfg).unwrap();
    let out = Document::load_mem(&bytes).unwrap();

    assert_eq!(output_intents_count(&src), 1);
    assert_eq!(output_intents_count(&out), 1);
}

#[test]
fn invalid_page_count() {
    let cfg = ImpositionConfig::default();
    let res = impose(b"%PDF-1.4\n", &cfg);
    assert!(res.is_err());
}

#[test]
fn png_one_page_output() {
    use image::{ImageBuffer, Rgb};
    use imposer::{impose_sources, sources_from_bytes};
    let img: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_pixel(120, 80, Rgb([255u8, 0, 0]));
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let sources = sources_from_bytes(&[png]).unwrap();
    let cfg = ImpositionConfig {
        sheet_size: SheetSize::A4,
        sheet_orientation: SheetOrientation::Auto,
        bleed_mm: 2.0,
        safety_margin_mm: 5.0,
        document_orientation: DocumentOrientation::Portrait,
        rotate_copies: true,
    };
    let bytes = impose_sources(&sources, &cfg).unwrap();
    let doc = Document::load_mem(&bytes).unwrap();
    assert_eq!(doc.get_pages().len(), 1);
}

#[test]
fn jpeg_two_page_output() {
    use image::{ImageBuffer, Rgb};
    use imposer::{impose_sources, sources_from_bytes};
    let mut inputs = Vec::new();
    for color in [[255u8, 0, 0], [0, 0, 255]] {
        let img: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_pixel(120, 80, Rgb(color));
        let mut jpg = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut jpg),
            image::ImageFormat::Jpeg,
        )
        .unwrap();
        inputs.push(jpg);
    }
    let sources = sources_from_bytes(&inputs).unwrap();
    let cfg = ImpositionConfig {
        sheet_size: SheetSize::Sra3,
        sheet_orientation: SheetOrientation::Auto,
        bleed_mm: 2.0,
        safety_margin_mm: 5.0,
        document_orientation: DocumentOrientation::Portrait,
        rotate_copies: true,
    };
    let bytes = impose_sources(&sources, &cfg).unwrap();
    let doc = Document::load_mem(&bytes).unwrap();
    assert_eq!(doc.get_pages().len(), 2);
}

#[test]
fn too_many_sources_error() {
    use image::{ImageBuffer, Rgb};
    use imposer::{impose_sources, sources_from_bytes};
    let mut inputs = Vec::new();
    for _ in 0..3 {
        let img: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_pixel(60, 40, Rgb([0u8, 255, 0]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        inputs.push(png);
    }
    let sources = sources_from_bytes(&inputs).unwrap();
    let cfg = ImpositionConfig::default();
    assert!(impose_sources(&sources, &cfg).is_err());
}

fn minimal_icc_profile() -> Vec<u8> {
    let mut b = vec![0u8; 128];
    // declared size = actual length
    b[0..4].copy_from_slice(&128u32.to_be_bytes());
    b[8] = 4; // ICC version major 4
    b[12..16].copy_from_slice(b"prtr");
    b[16..20].copy_from_slice(b"CMYK");
    b[20..24].copy_from_slice(b"Lab ");
    b[36..40].copy_from_slice(b"acsp");
    b
}

fn build_minimal_psd(width: u32, height: u32, profile: Option<&[u8]>) -> Vec<u8> {
    let mut b = Vec::new();
    // Header
    b.extend_from_slice(b"8BPS");
    b.extend_from_slice(&1u16.to_be_bytes()); // version
    b.extend_from_slice(&[0u8; 6]); // reserved
    b.extend_from_slice(&4u16.to_be_bytes()); // channels
    b.extend_from_slice(&height.to_be_bytes());
    b.extend_from_slice(&width.to_be_bytes());
    b.extend_from_slice(&8u16.to_be_bytes()); // depth
    b.extend_from_slice(&4u16.to_be_bytes()); // color mode = CMYK

    // Color mode data section: length + data
    b.extend_from_slice(&0u32.to_be_bytes());

    // Image resources section.
    let mut resources = Vec::new();
    if let Some(profile) = profile {
        resources.extend_from_slice(b"8BIM");
        resources.extend_from_slice(&0x040fu16.to_be_bytes()); // resource id = ICC profile
        resources.push(0); // empty Pascal name
        resources.push(0); // padding to even boundary
        resources.extend_from_slice(&(profile.len() as u32).to_be_bytes());
        resources.extend_from_slice(profile);
        if profile.len() % 2 == 1 {
            resources.push(0);
        }
    }
    b.extend_from_slice(&(resources.len() as u32).to_be_bytes());
    b.extend_from_slice(&resources);

    // Layer and mask information section: length 0
    b.extend_from_slice(&0u32.to_be_bytes());

    // Image data section: raw compression.
    b.extend_from_slice(&0u16.to_be_bytes());
    let pixels = (width as usize) * (height as usize) * 4usize;
    b.extend(std::iter::repeat_n(0u8, pixels));

    b
}

#[test]
fn psd_cmyk_single_page_output() {
    let profile = minimal_icc_profile();
    let psd = build_minimal_psd(120, 80, Some(&profile));
    let sources = sources_from_bytes(&[psd]).unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].color_kind(), SourceColorKind::Cmyk);
    assert_eq!(
        sources[0].color_label(),
        "CMJN (profil ICC embarqué)".to_string()
    );

    let cfg = ImpositionConfig {
        sheet_size: SheetSize::A4,
        sheet_orientation: SheetOrientation::Auto,
        bleed_mm: 2.0,
        safety_margin_mm: 5.0,
        document_orientation: DocumentOrientation::Portrait,
        rotate_copies: true,
    };
    let bytes = impose_sources(&sources, &cfg).unwrap();
    let doc = Document::load_mem(&bytes).unwrap();
    assert_eq!(doc.get_pages().len(), 1);

    let raw = String::from_utf8_lossy(&bytes);
    assert!(raw.contains("ICCBased"));
    assert!(raw.contains("GTS_PDFX"));
    assert!(raw.contains("PDF/X-4"));
}

#[test]
fn psd_cmyk_without_profile_is_rejected() {
    let psd = build_minimal_psd(8, 8, None);
    let res = sources_from_bytes(&[psd]);
    assert!(res.is_err(), "{:?}", res.err());
}

fn output_intents_count(doc: &Document) -> usize {
    let cat_id = match doc.trailer.get(b"Root") {
        Ok(lopdf::Object::Reference(id)) => *id,
        _ => return 0,
    };
    let cat = match doc.get_object(cat_id) {
        Ok(lopdf::Object::Dictionary(d)) => d,
        _ => return 0,
    };
    match cat.get(b"OutputIntents") {
        Ok(obj) => obj.as_array().map(|a| a.len()).unwrap_or(0),
        Err(_) => 0,
    }
}
