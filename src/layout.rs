use crate::config::{ImpositionConfig, SheetOrientation};
use crate::error::ImposerError;
use crate::geometry::mm_to_pt;

#[derive(Debug, Clone, Copy)]
pub struct Placement {
    pub trim_x: f32,
    pub trim_y: f32,
    pub trim_w: f32,
    pub trim_h: f32,
}

#[derive(Debug, Clone)]
pub struct LayoutPlan {
    pub sheet_width: f32,
    pub sheet_height: f32,
    pub safety_margin: f32,
    pub bleed: f32,
    pub source_width: f32,
    pub source_height: f32,
    pub item_rotated: bool,
    pub cols: usize,
    pub rows: usize,
    pub placements: Vec<Placement>,
}

pub fn compute_layout(
    source_w: f32,
    source_h: f32,
    config: &ImpositionConfig,
) -> Result<LayoutPlan, ImposerError> {
    let (base_w_mm, base_h_mm) = config.sheet_size.dimensions_mm();
    let base_w = mm_to_pt(base_w_mm);
    let base_h = mm_to_pt(base_h_mm);

    let sheet_candidates: Vec<(f32, f32)> = match config.sheet_orientation {
        SheetOrientation::Portrait => vec![(base_w, base_h)],
        SheetOrientation::Landscape => vec![(base_h, base_w)],
        SheetOrientation::Auto => vec![(base_w, base_h), (base_h, base_w)],
    };

    let bleed = mm_to_pt(config.bleed_mm);
    let safety = mm_to_pt(config.safety_margin_mm);

    if source_w <= 2.0 * bleed || source_h <= 2.0 * bleed {
        return Err(ImposerError::BleedTooLarge(format!(
            "document {}×{} pt, bleed {} pt",
            source_w, source_h, bleed
        )));
    }

    // The input page already includes the printable document plus its bleed:
    // its dimensions are therefore used directly as the size of one copy.
    let item_w0 = source_w;
    let item_h0 = source_h;

    let mut item_orientations: Vec<(f32, f32, bool)> = vec![(item_w0, item_h0, false)];
    if config.rotate_copies {
        item_orientations.push((item_h0, item_w0, true));
    }

    #[derive(Clone)]
    struct Best {
        sheet_w: f32,
        sheet_h: f32,
        item_w: f32,
        item_h: f32,
        item_rotated: bool,
        cols: usize,
        rows: usize,
        count: usize,
        waste: f32,
    }

    let mut best: Option<Best> = None;

    for &(sheet_w, sheet_h) in &sheet_candidates {
        let safe_w = sheet_w - 2.0 * safety;
        let safe_h = sheet_h - 2.0 * safety;
        if safe_w <= 0.0 || safe_h <= 0.0 {
            continue;
        }
        for &(iw, ih, rotated) in &item_orientations {
            if iw > safe_w || ih > safe_h {
                continue;
            }
            let cols = (safe_w / iw).floor() as usize;
            let rows = (safe_h / ih).floor() as usize;
            if cols == 0 || rows == 0 {
                continue;
            }
            let count = cols * rows;
            let waste = safe_w * safe_h - (count as f32) * iw * ih;
            let candidate = Best {
                sheet_w,
                sheet_h,
                item_w: iw,
                item_h: ih,
                item_rotated: rotated,
                cols,
                rows,
                count,
                waste,
            };
            let replace = match &best {
                None => true,
                Some(b) => count > b.count || (count == b.count && waste < b.waste),
            };
            if replace {
                best = Some(candidate);
            }
        }
    }

    let best = best.ok_or(ImposerError::NoFit)?;

    let safe_w = best.sheet_w - 2.0 * safety;
    let safe_h = best.sheet_h - 2.0 * safety;
    let grid_w = best.cols as f32 * best.item_w;
    let grid_h = best.rows as f32 * best.item_h;
    let origin_x = safety + (safe_w - grid_w) / 2.0;
    let origin_y = safety + (safe_h - grid_h) / 2.0;

    let mut placements = Vec::with_capacity(best.count);
    for row in 0..best.rows {
        for col in 0..best.cols {
            let cell_x = origin_x + col as f32 * best.item_w;
            let cell_y = origin_y + row as f32 * best.item_h;
            let trim_x = cell_x + bleed;
            let trim_y = cell_y + bleed;
            let (trim_w, trim_h) = if best.item_rotated {
                (source_h - 2.0 * bleed, source_w - 2.0 * bleed)
            } else {
                (source_w - 2.0 * bleed, source_h - 2.0 * bleed)
            };
            placements.push(Placement {
                trim_x,
                trim_y,
                trim_w,
                trim_h,
            });
        }
    }

    Ok(LayoutPlan {
        sheet_width: best.sheet_w,
        sheet_height: best.sheet_h,
        safety_margin: safety,
        bleed,
        source_width: source_w,
        source_height: source_h,
        item_rotated: best.item_rotated,
        cols: best.cols,
        rows: best.rows,
        placements,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ImpositionConfig, SheetOrientation, SheetSize};

    #[test]
    fn layout_produces_pages() {
        let cfg = ImpositionConfig {
            sheet_size: SheetSize::A4,
            sheet_orientation: SheetOrientation::Auto,
            bleed_mm: 2.0,
            safety_margin_mm: 5.0,
            document_orientation: crate::config::DocumentOrientation::Portrait,
            rotate_copies: true,
        };
        let src_w = crate::geometry::mm_to_pt(90.0);
        let src_h = crate::geometry::mm_to_pt(120.0);
        let plan = compute_layout(src_w, src_h, &cfg).unwrap();
        assert!(plan.cols >= 1 && plan.rows >= 1);
        assert!(!plan.placements.is_empty());
        assert_eq!(plan.placements.len(), plan.cols * plan.rows);
    }
}
