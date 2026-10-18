use lopdf::{Dictionary, Document, Object, Stream};
use std::fs;

fn main() {
    for (name, pages) in [("sample_1page", 1usize), ("sample_2page", 2usize)] {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let catalog_id = doc.new_object_id();

        let mut pages_dict = Dictionary::new();
        pages_dict.set("Type", Object::Name(b"Pages".to_vec()));
        pages_dict.set("Kids", Object::Array(vec![]));
        pages_dict.set("Count", Object::Integer(0));
        doc.objects.insert(pages_id, Object::Dictionary(pages_dict));

        // Profil colorimétrique factice (OutputIntent)
        let icc_id = doc.add_object(Stream::new(
            Dictionary::new(),
            b"dummy icc profile bytes".to_vec(),
        ));
        let mut oi = Dictionary::new();
        oi.set("Type", Object::Name(b"OutputIntent".to_vec()));
        oi.set("S", Object::Name(b"GTS_PDFA1".to_vec()));
        oi.set(
            "OutputConditionIdentifier",
            Object::String(b"sRGB".to_vec(), lopdf::StringFormat::Literal),
        );
        oi.set("DestOutputProfile", Object::Reference(icc_id));
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference(pages_id));
        catalog.set("OutputIntents", Object::Array(vec![Object::Dictionary(oi)]));
        doc.objects.insert(catalog_id, Object::Dictionary(catalog));
        doc.trailer.set("Root", Object::Reference(catalog_id));

        let mut kids = Vec::new();
        for i in 0..pages {
            let page_id = doc.new_object_id();
            let (r, g, b) = if i == 0 {
                (0.2, 0.4, 0.8)
            } else {
                (0.8, 0.3, 0.2)
            };
            let content = format!("q {} {} {} rg 10 10 220 320 re f Q\n", r, g, b);
            let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
            let mut page = Dictionary::new();
            page.set("Type", Object::Name(b"Page".to_vec()));
            page.set("Parent", Object::Reference(pages_id));
            page.set(
                "MediaBox",
                Object::Array(vec![0.into(), 0.into(), 250.into(), 350.into()]),
            );
            page.set("Contents", Object::Reference(content_id));
            page.set("Resources", Object::Dictionary(Dictionary::new()));
            doc.objects.insert(page_id, Object::Dictionary(page));
            kids.push(Object::Reference(page_id));
        }

        if let Some(Object::Dictionary(p)) = doc.objects.get_mut(&pages_id) {
            p.set("Kids", Object::Array(kids));
            p.set("Count", Object::Integer(pages as i64));
            p.set(
                "MediaBox",
                Object::Array(vec![0.into(), 0.into(), 250.into(), 350.into()]),
            );
        }

        let out_dir = "tests/fixtures";
        fs::create_dir_all(out_dir).unwrap();
        let out = format!("{}/{}.pdf", out_dir, name);
        doc.save_to(&mut fs::File::create(&out).unwrap()).unwrap();
        println!("wrote {}", out);
    }
}
