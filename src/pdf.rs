use std::collections::HashMap;

use lopdf::{
    Dictionary, Document, Object, ObjectId, Stream,
    content::{Content, Operation},
};

use crate::config::ImpositionConfig;
use crate::error::{ImposerError, Result};
use crate::geometry::mm_to_pt;
use crate::layout::{LayoutPlan, compute_layout};

pub fn impose(input: &[u8], config: &ImpositionConfig) -> Result<Vec<u8>> {
    let source = Document::load_mem(input).map_err(|e| ImposerError::Parse(e.to_string()))?;

    let pages_map = source.get_pages();
    let num_pages = pages_map.len();
    if num_pages != 1 && num_pages != 2 {
        return Err(ImposerError::InvalidPageCount(num_pages));
    }

    let mut page_list: Vec<(u32, ObjectId)> = pages_map.into_iter().collect();
    page_list.sort_by_key(|(n, _)| *n);

    let first_page_id = page_list[0].1;
    let (source_w, source_h) = get_page_dimensions(&source, first_page_id)?;
    let plan = compute_layout(source_w, source_h, config)?;

    let mut output = Document::with_version("1.7");

    let pages_id = output.new_object_id();
    let catalog_id = output.new_object_id();

    let mut pages_dict = Dictionary::new();
    pages_dict.set("Type", Object::Name(b"Pages".to_vec()));
    pages_dict.set("Kids", Object::Array(Vec::new()));
    pages_dict.set("Count", Object::Integer(0));
    output
        .objects
        .insert(pages_id, Object::Dictionary(pages_dict));

    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    output
        .objects
        .insert(catalog_id, Object::Dictionary(catalog));
    output.trailer.set("Root", Object::Reference(catalog_id));

    copy_output_intents(&source, &mut output, catalog_id)?;

    let mut cache: HashMap<ObjectId, ObjectId> = HashMap::new();
    let mut page_refs: Vec<Object> = Vec::new();

    for (face_index, (_, source_page_id)) in page_list.iter().enumerate() {
        let is_verso = face_index == 1;
        let xobj_id = create_form_xobject(
            &mut output,
            &source,
            *source_page_id,
            source_w,
            source_h,
            &mut cache,
        )?;

        let mut xobjects = Dictionary::new();
        xobjects.set(b"Page0".to_vec(), Object::Reference(xobj_id));
        let mut resources = Dictionary::new();
        resources.set(b"XObject".to_vec(), Object::Dictionary(xobjects));

        let verso_rotated =
            is_verso && config.document_orientation == crate::config::DocumentOrientation::Portrait;
        let content_data = build_face_content(&plan, is_verso, verso_rotated)?;
        let content_id = output.add_object(Stream::new(Dictionary::new(), content_data));

        let mut page_dict = Dictionary::new();
        page_dict.set("Type", Object::Name(b"Page".to_vec()));
        page_dict.set("Parent", Object::Reference(pages_id));
        page_dict.set(
            "MediaBox",
            Object::Array(vec![
                0.into(),
                0.into(),
                plan.sheet_width.into(),
                plan.sheet_height.into(),
            ]),
        );
        page_dict.set("Contents", Object::Reference(content_id));
        page_dict.set("Resources", Object::Dictionary(resources));

        let sheet_page_id = output.add_object(page_dict);
        page_refs.push(Object::Reference(sheet_page_id));
    }

    let mut final_pages = Dictionary::new();
    final_pages.set("Type", Object::Name(b"Pages".to_vec()));
    final_pages.set("Kids", Object::Array(page_refs));
    final_pages.set("Count", Object::Integer(num_pages as i64));
    final_pages.set(
        "MediaBox",
        Object::Array(vec![
            0.into(),
            0.into(),
            plan.sheet_width.into(),
            plan.sheet_height.into(),
        ]),
    );
    output
        .objects
        .insert(pages_id, Object::Dictionary(final_pages));

    let mut bytes = Vec::new();
    output
        .save_to(&mut bytes)
        .map_err(|e| ImposerError::Generation(e.to_string()))?;
    Ok(bytes)
}

fn get_page_dimensions(doc: &Document, page_id: ObjectId) -> Result<(f32, f32)> {
    let page_obj = doc
        .get_object(page_id)
        .map_err(|e| ImposerError::Dimensions(e.to_string()))?;
    let page_dict = page_obj
        .as_dict()
        .map_err(|e| ImposerError::Dimensions(e.to_string()))?;

    // Page size for imposition is always the full PDF page (MediaBox),
    // because the input already includes any bleed.
    let media_box = page_dict
        .get(b"MediaBox")
        .and_then(|obj| obj.as_array())
        .map_err(|e| ImposerError::Dimensions(e.to_string()))?;

    if media_box.len() < 4 {
        return Err(ImposerError::Dimensions("MediaBox incomplete".to_string()));
    }

    let x1 = media_box[0].as_float().unwrap_or(0.0);
    let y1 = media_box[1].as_float().unwrap_or(0.0);
    let x2 = media_box[2].as_float().unwrap_or(0.0);
    let y2 = media_box[3].as_float().unwrap_or(0.0);

    Ok((x2 - x1, y2 - y1))
}

fn build_face_content(plan: &LayoutPlan, is_verso: bool, rotate_verso: bool) -> Result<Vec<u8>> {
    let mut operations: Vec<Operation> = Vec::new();

    if is_verso && rotate_verso {
        operations.push(Operation::new("q", vec![]));
        operations.push(Operation::new(
            "cm",
            vec![
                (-1.0_f32).into(),
                0.0.into(),
                0.0.into(),
                (-1.0_f32).into(),
                plan.sheet_width.into(),
                plan.sheet_height.into(),
            ],
        ));
    }

    for p in &plan.placements {
        operations.push(Operation::new("q", vec![]));
        let cell_x = p.trim_x - plan.bleed;
        let cell_y = p.trim_y - plan.bleed;
        if plan.item_rotated {
            operations.push(Operation::new(
                "cm",
                vec![
                    0.0.into(),
                    1.0.into(),
                    (-1.0_f32).into(),
                    0.0.into(),
                    (cell_x + p.trim_w + plan.bleed * 2.0).into(), // source width horizontal after rotation
                    cell_y.into(),
                ],
            ));
        } else {
            operations.push(Operation::new(
                "cm",
                vec![
                    1.0.into(),
                    0.0.into(),
                    0.0.into(),
                    1.0.into(),
                    cell_x.into(),
                    cell_y.into(),
                ],
            ));
        }
        operations.push(Operation::new("Do", vec![Object::Name(b"Page0".to_vec())]));
        operations.push(Operation::new("Q", vec![]));
    }

    draw_crop_marks(&mut operations, plan)?;

    if is_verso && rotate_verso {
        operations.push(Operation::new("Q", vec![]));
    }

    let content = Content { operations };
    content
        .encode()
        .map_err(|e| ImposerError::Generation(e.to_string()))
}

pub fn dedup_coords(mut coords: Vec<f32>, tol: f32) -> Vec<f32> {
    coords.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<f32> = Vec::new();
    for c in coords {
        if out.last().is_none_or(|last| (c - *last).abs() > tol) {
            out.push(c);
        }
    }
    out
}

pub fn crop_mark_edges(plan: &LayoutPlan) -> (Vec<f32>, Vec<f32>) {
    if plan.placements.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let tol = mm_to_pt(0.01);
    let xs: Vec<f32> = plan
        .placements
        .iter()
        .flat_map(|p| [p.trim_x, p.trim_x + p.trim_w])
        .collect();
    let ys: Vec<f32> = plan
        .placements
        .iter()
        .flat_map(|p| [p.trim_y, p.trim_y + p.trim_h])
        .collect();
    (dedup_coords(xs, tol), dedup_coords(ys, tol))
}

pub fn crop_mark_count(plan: &LayoutPlan) -> usize {
    let (xs, ys) = crop_mark_edges(plan);
    2 * (xs.len() + ys.len())
}

pub fn crop_marks_count(plan: &LayoutPlan) -> usize {
    crop_mark_count(plan)
}

pub fn count_crop_marks(plan: &LayoutPlan) -> usize {
    crop_mark_count(plan)
}

pub fn crop_marks(plan: &LayoutPlan) -> (Vec<f32>, Vec<f32>) {
    crop_mark_edges(plan)
}

pub fn draw_crop_marks(ops: &mut Vec<Operation>, plan: &LayoutPlan) -> Result<()> {
    if plan.placements.is_empty() {
        return Ok(());
    }

    let tol = mm_to_pt(0.01);

    let mut xs: Vec<f32> = Vec::new();
    let mut ys: Vec<f32> = Vec::new();
    for p in &plan.placements {
        xs.push(p.trim_x);
        xs.push(p.trim_x + p.trim_w);
        ys.push(p.trim_y);
        ys.push(p.trim_y + p.trim_h);
    }
    let xs = dedup_coords(xs, tol);
    let ys = dedup_coords(ys, tol);

    let xmin = plan
        .placements
        .iter()
        .map(|p| p.trim_x)
        .fold(f32::INFINITY, f32::min);
    let xmax = plan
        .placements
        .iter()
        .map(|p| p.trim_x + p.trim_w)
        .fold(f32::NEG_INFINITY, f32::max);
    let ymin = plan
        .placements
        .iter()
        .map(|p| p.trim_y)
        .fold(f32::INFINITY, f32::min);
    let ymax = plan
        .placements
        .iter()
        .map(|p| p.trim_y + p.trim_h)
        .fold(f32::NEG_INFINITY, f32::max);

    let length = mm_to_pt(3.0);
    let offset = mm_to_pt(3.0).max(plan.bleed);

    let eps = mm_to_pt(0.01);
    if xmax + offset + length > plan.sheet_width + eps
        || xmin - offset - length < -eps
        || ymax + offset + length > plan.sheet_height + eps
        || ymin - offset - length < -eps
    {
        return Err(ImposerError::CropMarksMargin(format!(
            "sheet {}x{} pt, poses bbox [{},{}]x[{},{}], offset {} pt, length {} pt",
            plan.sheet_width, plan.sheet_height, xmin, ymin, xmax, ymax, offset, length
        )));
    }

    ops.push(Operation::new("q", vec![]));
    ops.push(Operation::new("w", vec![0.25_f32.into()]));
    ops.push(Operation::new(
        "K",
        vec![
            1.0_f32.into(),
            1.0_f32.into(),
            1.0_f32.into(),
            1.0_f32.into(),
        ],
    ));

    for x in &xs {
        // top
        ops.push(Operation::new(
            "m",
            vec![(*x).into(), (ymax + offset).into()],
        ));
        ops.push(Operation::new(
            "l",
            vec![(*x).into(), (ymax + offset + length).into()],
        ));
        // bottom
        ops.push(Operation::new(
            "m",
            vec![(*x).into(), (ymin - offset).into()],
        ));
        ops.push(Operation::new(
            "l",
            vec![(*x).into(), (ymin - offset - length).into()],
        ));
    }

    for y in &ys {
        // left
        ops.push(Operation::new(
            "m",
            vec![(xmin - offset).into(), (*y).into()],
        ));
        ops.push(Operation::new(
            "l",
            vec![(xmin - offset - length).into(), (*y).into()],
        ));
        // right
        ops.push(Operation::new(
            "m",
            vec![(xmax + offset).into(), (*y).into()],
        ));
        ops.push(Operation::new(
            "l",
            vec![(xmax + offset + length).into(), (*y).into()],
        ));
    }

    ops.push(Operation::new("S", vec![]));
    ops.push(Operation::new("Q", vec![]));

    Ok(())
}

fn copy_output_intents(source: &Document, dest: &mut Document, catalog_id: ObjectId) -> Result<()> {
    let src_catalog_id = match source.trailer.get(b"Root") {
        Ok(Object::Reference(id)) => *id,
        _ => return Ok(()),
    };
    let src_catalog = match source.get_object(src_catalog_id) {
        Ok(Object::Dictionary(d)) => d,
        _ => return Ok(()),
    };

    let intents = match src_catalog.get(b"OutputIntents") {
        Ok(o) => o.clone(),
        Err(_) => return Ok(()),
    };

    let mut cache: HashMap<ObjectId, ObjectId> = HashMap::new();
    let copied = copy_object_deep(source, dest, &intents, &mut cache)?;

    if let Some(Object::Dictionary(dict)) = dest.objects.get_mut(&catalog_id) {
        dict.set(b"OutputIntents".to_vec(), copied);
    }
    Ok(())
}

fn create_form_xobject(
    output_doc: &mut Document,
    source_doc: &Document,
    source_page_id: ObjectId,
    width: f32,
    height: f32,
    cache: &mut HashMap<ObjectId, ObjectId>,
) -> Result<ObjectId> {
    let page_obj = source_doc
        .get_object(source_page_id)
        .map_err(|e| ImposerError::Parse(e.to_string()))?;
    let page_dict = page_obj
        .as_dict()
        .map_err(|e| ImposerError::Parse(e.to_string()))?;

    let contents = page_dict
        .get(b"Contents")
        .map_err(|_| ImposerError::Parse("Page has no contents".to_string()))?;

    let content_data = match contents {
        Object::Reference(ref_id) => {
            let content_obj = source_doc
                .get_object(*ref_id)
                .map_err(|e| ImposerError::Parse(e.to_string()))?;
            match content_obj {
                Object::Stream(stream) => stream
                    .decompressed_content()
                    .map_err(|e| ImposerError::Parse(e.to_string()))?,
                _ => return Err(ImposerError::Parse("Content is not a stream".to_string())),
            }
        }
        Object::Array(refs) => {
            let mut combined = Vec::new();
            for obj in refs {
                if let Object::Reference(ref_id) = obj
                    && let Ok(Object::Stream(stream)) = source_doc.get_object(*ref_id)
                    && let Ok(decompressed) = stream.decompressed_content()
                {
                    combined.extend_from_slice(&decompressed);
                }
            }
            combined
        }
        _ => return Err(ImposerError::Parse("Unexpected content type".to_string())),
    };

    let resources = if let Ok(res) = page_dict.get(b"Resources") {
        copy_object_deep(source_doc, output_doc, res, cache)?
    } else {
        Object::Dictionary(Dictionary::new())
    };

    let mut xobj_dict = Dictionary::new();
    xobj_dict.set("Type", Object::Name(b"XObject".to_vec()));
    xobj_dict.set("Subtype", Object::Name(b"Form".to_vec()));
    xobj_dict.set(
        "BBox",
        Object::Array(vec![0.into(), 0.into(), width.into(), height.into()]),
    );
    xobj_dict.set("Resources", resources);

    let xobj_stream = Stream::new(xobj_dict, content_data);
    let xobj_id = output_doc.add_object(xobj_stream);
    Ok(xobj_id)
}

fn copy_object_deep(
    source_doc: &Document,
    dest_doc: &mut Document,
    obj: &Object,
    cache: &mut HashMap<ObjectId, ObjectId>,
) -> Result<Object> {
    match obj {
        Object::Reference(ref_id) => {
            if let Some(&new_id) = cache.get(ref_id) {
                return Ok(Object::Reference(new_id));
            }
            if let Ok(referenced_obj) = source_doc.get_object(*ref_id) {
                let copied = copy_object_deep(source_doc, dest_doc, referenced_obj, cache)?;
                let new_id = dest_doc.add_object(copied);
                cache.insert(*ref_id, new_id);
                Ok(Object::Reference(new_id))
            } else {
                Ok(Object::Null)
            }
        }
        Object::Dictionary(dict) => {
            let mut new_dict = Dictionary::new();
            for (key, value) in dict {
                let copied_value = copy_object_deep(source_doc, dest_doc, value, cache)?;
                new_dict.set(key.clone(), copied_value);
            }
            Ok(Object::Dictionary(new_dict))
        }
        Object::Array(arr) => {
            let mut new_arr = Vec::new();
            for item in arr {
                new_arr.push(copy_object_deep(source_doc, dest_doc, item, cache)?);
            }
            Ok(Object::Array(new_arr))
        }
        Object::Stream(stream) => {
            let mut new_dict = Dictionary::new();
            for (key, value) in &stream.dict {
                let copied_value = copy_object_deep(source_doc, dest_doc, value, cache)?;
                new_dict.set(key.clone(), copied_value);
            }
            Ok(Object::Stream(Stream {
                dict: new_dict,
                content: stream.content.clone(),
                allows_compression: stream.allows_compression,
                start_position: None,
            }))
        }
        _ => Ok(obj.clone()),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{LayoutPlan, Placement};

    fn plan(placements: Vec<Placement>) -> LayoutPlan {
        LayoutPlan {
            sheet_width: 1000.0,
            sheet_height: 1000.0,
            safety_margin: 30.0,
            bleed: 0.0,
            source_width: 10.0,
            source_height: 10.0,
            item_rotated: false,
            cols: 0,
            rows: 0,
            placements,
        }
    }

    #[test]
    fn dedup_exact_edges() {
        let coords = vec![0.0, 0.0, 10.0, 10.0001, 20.0];
        let out = dedup_coords(coords, mm_to_pt(0.01));
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn count_one_by_one_no_gutter() {
        let placements = vec![Placement {
            trim_x: 0.0,
            trim_y: 0.0,
            trim_w: 10.0,
            trim_h: 10.0,
        }];
        let p = plan(placements);
        assert_eq!(crop_mark_count(&p), 8);
    }

    #[test]
    fn count_two_by_two_no_gutter() {
        // adjacent trim boxes share edges
        let placements = vec![
            Placement {
                trim_x: 0.0,
                trim_y: 0.0,
                trim_w: 10.0,
                trim_h: 10.0,
            },
            Placement {
                trim_x: 10.0,
                trim_y: 0.0,
                trim_w: 10.0,
                trim_h: 10.0,
            },
            Placement {
                trim_x: 0.0,
                trim_y: 10.0,
                trim_w: 10.0,
                trim_h: 10.0,
            },
            Placement {
                trim_x: 10.0,
                trim_y: 10.0,
                trim_w: 10.0,
                trim_h: 10.0,
            },
        ];
        let p = plan(placements);
        assert_eq!(crop_mark_count(&p), 12);
    }

    #[test]
    fn count_two_by_four_with_gutter() {
        // 2 rows x 4 cols, separated edges (gutter)
        let mut placements = Vec::new();
        for r in 0..2 {
            for c in 0..4 {
                placements.push(Placement {
                    trim_x: c as f32 * 15.0,
                    trim_y: r as f32 * 15.0,
                    trim_w: 10.0,
                    trim_h: 10.0,
                });
            }
        }
        let p = plan(placements);
        assert_eq!(crop_mark_count(&p), 24);
    }

    #[test]
    fn count_four_by_two_no_gutter() {
        let mut placements = Vec::new();
        for r in 0..4 {
            for c in 0..2 {
                placements.push(Placement {
                    trim_x: c as f32 * 10.0,
                    trim_y: r as f32 * 10.0,
                    trim_w: 10.0,
                    trim_h: 10.0,
                });
            }
        }
        let p = plan(placements);
        // 4 rows x 2 cols => X=3, Y=5 => 16
        assert_eq!(crop_mark_count(&p), 16);
    }

    #[test]
    fn count_three_by_three_no_gutter() {
        let mut placements = Vec::new();
        for r in 0..3 {
            for c in 0..3 {
                placements.push(Placement {
                    trim_x: c as f32 * 10.0,
                    trim_y: r as f32 * 10.0,
                    trim_w: 10.0,
                    trim_h: 10.0,
                });
            }
        }
        let p = plan(placements);
        // X=4,Y=4 => 16
        assert_eq!(crop_mark_count(&p), 16);
    }

    #[test]
    fn draw_count_matches_marks() {
        let placements = vec![
            Placement {
                trim_x: 100.0,
                trim_y: 100.0,
                trim_w: 10.0,
                trim_h: 10.0,
            },
            Placement {
                trim_x: 110.0,
                trim_y: 100.0,
                trim_w: 10.0,
                trim_h: 10.0,
            },
        ];
        let p = plan(placements);
        let mut ops = Vec::new();
        draw_crop_marks(&mut ops, &p).unwrap();
        let m_count = ops.iter().filter(|o| o.operator == "m").count();
        assert_eq!(m_count, crop_mark_count(&p));
    }

    #[test]
    fn margin_error() {
        let mut p = plan(vec![Placement {
            trim_x: 0.0,
            trim_y: 0.0,
            trim_w: 10.0,
            trim_h: 10.0,
        }]);
        p.sheet_width = mm_to_pt(10.0);
        p.sheet_height = mm_to_pt(10.0);
        p.safety_margin = 0.0;
        let mut ops = Vec::new();
        assert!(draw_crop_marks(&mut ops, &p).is_err());
    }
}
