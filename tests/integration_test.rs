use imposer::config::{DocumentOrientation, SheetOrientation, SheetSize};
use imposer::{ImpositionConfig, impose};
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
