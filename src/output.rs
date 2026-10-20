//! Construction du PDF imposé conforme PDF/X-4.

use crate::error::{ImposerError, Result};
use crate::icc::IccProfile;
use crate::import::ImportedPage;
use crate::layout::{Placement, SheetSpec};
use sha2::{Digest, Sha256};

pub use crate::import::BoxSource;

#[derive(Debug, Clone)]
pub struct OutputOptions {
    pub title: String,
    pub producer: String,
    pub creation_date: String,
    pub mod_date: String,
    pub source_bleed_pt: f64,
    pub box_source: BoxSource,
    pub output_condition_identifier: Option<String>,
    pub registry_name: Option<String>,
    pub info_text: Option<String>,
    pub compression_xref_stream: bool,
    pub id_seed: Option<[u8; 16]>,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            title: "Imposed".to_string(),
            producer: "imposer".to_string(),
            creation_date: "D:20260101000000Z".to_string(),
            mod_date: "D:20260101000000Z".to_string(),
            source_bleed_pt: crate::geometry::mm_to_pt_f64(2.0),
            box_source: BoxSource::Bleed,
            output_condition_identifier: None,
            registry_name: None,
            info_text: None,
            compression_xref_stream: false,
            id_seed: None,
        }
    }
}

pub struct OutputBuilder {
    pub document: lopdf::Document,
    pub profile: IccProfile,
    pub options: OutputOptions,
    pub catalog_id: lopdf::ObjectId,
    pub pages_id: lopdf::ObjectId,
    pub pages: Vec<lopdf::ObjectId>,
    pub imported: Vec<ImportedPage>,
    pub resource_cache: std::collections::BTreeMap<(u32, u16), lopdf::ObjectId>,
    pub box_source: BoxSource,
}

impl OutputBuilder {
    pub fn new(profile: IccProfile, options: OutputOptions) -> Self {
        let mut doc = lopdf::Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let catalog_id = doc.new_object_id();
        let mut pages_dict = lopdf::Dictionary::new();
        pages_dict.set("Type", lopdf::Object::Name(b"Pages".to_vec()));
        pages_dict.set("Kids", lopdf::Object::Array(Vec::new()));
        pages_dict.set("Count", lopdf::Object::Integer(0));
        doc.objects
            .insert(pages_id, lopdf::Object::Dictionary(pages_dict));
        let mut catalog = lopdf::Dictionary::new();
        catalog.set("Type", lopdf::Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", lopdf::Object::Reference(pages_id));
        doc.objects
            .insert(catalog_id, lopdf::Object::Dictionary(catalog));
        doc.trailer
            .set("Root", lopdf::Object::Reference(catalog_id));
        let box_source = options.box_source;
        Self {
            document: doc,
            profile,
            options,
            catalog_id,
            pages_id,
            pages: Vec::new(),
            imported: Vec::new(),
            resource_cache: std::collections::BTreeMap::new(),
            box_source,
        }
    }

    pub fn register_imported(&mut self, ip: ImportedPage) {
        self.imported.push(ip);
    }

    pub fn add_sheet(
        &mut self,
        spec: &SheetSpec,
        placements: &[Placement],
    ) -> Result<lopdf::ObjectId> {
        self.add_sheet_with(spec, placements, 0)
    }

    pub fn add_sheet_with(
        &mut self,
        spec: &SheetSpec,
        placements: &[Placement],
        imported_index: usize,
    ) -> Result<lopdf::ObjectId> {
        let form = self
            .imported
            .get(imported_index)
            .ok_or_else(|| ImposerError::Generation("aucune page importée".to_string()))?
            .clone();
        let form_name = format!("Fm{}", imported_index);
        let mut xobjects = lopdf::Dictionary::new();
        xobjects.set(
            form_name.as_bytes().to_vec(),
            lopdf::Object::Reference(form.form_id),
        );

        let w = spec.width_pt;
        let h = spec.height_pt;
        let bleed = self.options.source_bleed_pt;
        let mut ops: Vec<lopdf::content::Operation> = Vec::new();
        for p in placements {
            ops.push(lopdf::content::Operation::new("q", vec![]));
            let cx = p.trim_x as f64 - bleed;
            let cy = p.trim_y as f64 - bleed;
            ops.push(lopdf::content::Operation::new(
                "cm",
                vec![
                    1.0.into(),
                    0.0.into(),
                    0.0.into(),
                    1.0.into(),
                    cx.into(),
                    cy.into(),
                ],
            ));
            ops.push(lopdf::content::Operation::new(
                "Do",
                vec![lopdf::Object::Name(form_name.as_bytes().to_vec())],
            ));
            ops.push(lopdf::content::Operation::new("Q", vec![]));
        }

        // Crop marks: build a LayoutPlan equivalent
        let plan = build_plan_from_sheet(spec, placements, self.options.source_bleed_pt as f32);
        let _ = crate::pdf::draw_crop_marks(&mut ops, &plan);

        let content_data = {
            let content = lopdf::content::Content { operations: ops };
            content
                .encode()
                .map_err(|e| ImposerError::Generation(e.to_string()))?
        };

        let content_id = self.document.add_object(lopdf::Stream {
            dict: lopdf::Dictionary::new(),
            content: content_data,
            allows_compression: true,
            start_position: None,
        });

        // TrimBox etc
        let media_box = lopdf::Object::Array(vec![0.0.into(), 0.0.into(), w.into(), h.into()]);
        let bleed_box = lopdf::Object::Array(vec![0.0.into(), 0.0.into(), w.into(), h.into()]);
        let trim_x = spec.trim_x_pt;
        let trim_y = spec.trim_y_pt;
        let trim_w = spec.trim_w_pt;
        let trim_h = spec.trim_h_pt;
        let trim_box = lopdf::Object::Array(vec![
            trim_x.into(),
            trim_y.into(),
            (trim_x + trim_w).into(),
            (trim_y + trim_h).into(),
        ]);

        let mut resources = lopdf::Dictionary::new();
        resources.set("XObject", lopdf::Object::Dictionary(xobjects));

        let mut page_dict = lopdf::Dictionary::new();
        page_dict.set("Type", lopdf::Object::Name(b"Page".to_vec()));
        page_dict.set("Parent", lopdf::Object::Reference(self.pages_id));
        page_dict.set("MediaBox", media_box);
        page_dict.set("BleedBox", bleed_box);
        page_dict.set("TrimBox", trim_box);
        page_dict.set("Contents", lopdf::Object::Reference(content_id));
        page_dict.set("Resources", lopdf::Object::Dictionary(resources));

        let page_id = self.document.add_object(page_dict);
        self.pages.push(page_id);
        Ok(page_id)
    }

    pub fn finish(mut self) -> Result<Vec<u8>> {
        // Pages
        let mut pages_dict = lopdf::Dictionary::new();
        pages_dict.set("Type", lopdf::Object::Name(b"Pages".to_vec()));
        pages_dict.set(
            "Kids",
            lopdf::Object::Array(
                self.pages
                    .iter()
                    .map(|id| lopdf::Object::Reference(*id))
                    .collect(),
            ),
        );
        pages_dict.set("Count", lopdf::Object::Integer(self.pages.len() as i64));
        self.document
            .objects
            .insert(self.pages_id, lopdf::Object::Dictionary(pages_dict));

        // OutputIntents
        let icc_stream_id = self.add_icc_stream()?;
        let ocid = self
            .options
            .output_condition_identifier
            .clone()
            .unwrap_or_else(|| {
                self.profile
                    .info
                    .description
                    .clone()
                    .unwrap_or_else(|| "Custom CMYK".to_string())
            });
        let info_txt = self
            .options
            .info_text
            .clone()
            .unwrap_or_else(|| ocid.clone());
        let registry = self
            .options
            .registry_name
            .clone()
            .unwrap_or_else(|| "http://www.color.org".to_string());

        let mut intent = lopdf::Dictionary::new();
        intent.set("Type", lopdf::Object::Name(b"OutputIntent".to_vec()));
        intent.set("S", lopdf::Object::Name(b"GTS_PDFX".to_vec()));
        intent.set(
            "OutputConditionIdentifier",
            lopdf::Object::String(ocid.as_bytes().to_vec(), lopdf::StringFormat::Literal),
        );
        if !registry.is_empty() {
            intent.set(
                "RegistryName",
                lopdf::Object::String(registry.as_bytes().to_vec(), lopdf::StringFormat::Literal),
            );
        }
        intent.set(
            "Info",
            lopdf::Object::String(info_txt.as_bytes().to_vec(), lopdf::StringFormat::Literal),
        );
        intent.set("DestOutputProfile", lopdf::Object::Reference(icc_stream_id));
        let intent_id = self.document.add_object(lopdf::Object::Dictionary(intent));

        // Add to catalog
        if let Some(cat) = self
            .document
            .objects
            .get_mut(&self.catalog_id)
            .and_then(|o| o.as_dict_mut().ok())
        {
            cat.set(
                "OutputIntents",
                lopdf::Object::Array(vec![lopdf::Object::Reference(intent_id)]),
            );
        }

        // Metadata stream (XMP)
        let xmp = build_xmp(&self.options);
        let mut md_dict = lopdf::Dictionary::new();
        md_dict.set("Type", lopdf::Object::Name(b"Metadata".to_vec()));
        md_dict.set("Subtype", lopdf::Object::Name(b"XML".to_vec()));
        let md_id = self.document.add_object(lopdf::Stream {
            dict: md_dict,
            content: xmp.into_bytes(),
            allows_compression: false,
            start_position: None,
        });
        if let Some(cat) = self
            .document
            .objects
            .get_mut(&self.catalog_id)
            .and_then(|o| o.as_dict_mut().ok())
        {
            cat.set("Metadata", lopdf::Object::Reference(md_id));
        }

        // Info dict
        let mut info = lopdf::Dictionary::new();
        info.set(
            "Title",
            lopdf::Object::String(
                self.options.title.as_bytes().to_vec(),
                lopdf::StringFormat::Literal,
            ),
        );
        info.set(
            "Producer",
            lopdf::Object::String(
                self.options.producer.as_bytes().to_vec(),
                lopdf::StringFormat::Literal,
            ),
        );
        info.set(
            "CreationDate",
            lopdf::Object::String(
                self.options.creation_date.as_bytes().to_vec(),
                lopdf::StringFormat::Literal,
            ),
        );
        info.set(
            "ModDate",
            lopdf::Object::String(
                self.options.mod_date.as_bytes().to_vec(),
                lopdf::StringFormat::Literal,
            ),
        );
        info.set(
            "GTS_PDFXVersion",
            lopdf::Object::String(b"PDF/X-4".to_vec(), lopdf::StringFormat::Literal),
        );
        info.set("Trapped", lopdf::Object::Name(b"False".to_vec()));
        let info_id = self.document.add_object(info);
        self.document
            .trailer
            .set("Info", lopdf::Object::Reference(info_id));

        // /ID
        let id_bytes = self
            .options
            .id_seed
            .unwrap_or_else(|| derive_id(&self.options, &self.profile));
        self.document.trailer.set(
            "ID",
            lopdf::Object::Array(vec![
                lopdf::Object::String(id_bytes.to_vec(), lopdf::StringFormat::Literal),
                lopdf::Object::String(id_bytes.to_vec(), lopdf::StringFormat::Literal),
            ]),
        );

        let mut bytes = Vec::new();
        self.document
            .save_to(&mut bytes)
            .map_err(|e| ImposerError::Generation(e.to_string()))?;
        Ok(bytes)
    }

    pub fn write_to(self, path: &std::path::Path) -> Result<()> {
        let bytes = self.finish()?;
        std::fs::write(path, bytes).map_err(ImposerError::Io)
    }

    fn add_icc_stream(&mut self) -> Result<lopdf::ObjectId> {
        let mut dict = lopdf::Dictionary::new();
        dict.set("Type", lopdf::Object::Name(b"ICCProfile".to_vec()));
        dict.set("N", lopdf::Object::Integer(4));
        // Let profile size? optional
        let id = self.document.add_object(lopdf::Stream {
            dict,
            content: self.profile.bytes.clone(),
            allows_compression: false,
            start_position: None,
        });
        Ok(id)
    }
}

fn derive_id(options: &OutputOptions, profile: &IccProfile) -> [u8; 16] {
    let mut hasher = Sha256::new();
    hasher.update(options.title.as_bytes());
    hasher.update(options.creation_date.as_bytes());
    hasher.update(profile.info.sha256);
    let out = hasher.finalize();
    let mut arr = [0u8; 16];
    arr.copy_from_slice(&out[..16]);
    arr
}

fn build_xmp(options: &OutputOptions) -> String {
    let begin = "\u{feff}";
    format!(
        "<?xpacket begin=\"{begin}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         <rdf:Description rdf:about=\"\"\n\
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
         xmlns:pdf=\"http://ns.adobe.com/pdf/1.3/\"\n\
         xmlns:pdfxid=\"http://www.npes.org/pdfx/ns/id/\">\n\
         <dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{t}</rdf:li></rdf:Alt></dc:title>\n\
         <pdf:Producer>{p}</pdf:Producer>\n\
         <pdfxid:GTS_PDFXVersion>PDF/X-4</pdfxid:GTS_PDFXVersion>\n\
         </rdf:Description>\n\
         </rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end=\"w\"?>",
        begin = begin,
        t = options.title,
        p = options.producer,
    )
}

fn build_plan_from_sheet(
    spec: &SheetSpec,
    placements: &[Placement],
    bleed: f32,
) -> crate::layout::LayoutPlan {
    crate::layout::LayoutPlan {
        sheet_width: spec.width_pt as f32,
        sheet_height: spec.height_pt as f32,
        safety_margin: 0.0,
        bleed,
        source_width: 0.0,
        source_height: 0.0,
        item_rotated: false,
        cols: 0,
        rows: 0,
        placements: placements.to_vec(),
    }
}

// ImportedPage must be cloneable for convenience
// (already derived Clone in import.rs)

#[allow(dead_code)]
fn _sheet_spec_use(_s: &SheetSpec) {}
