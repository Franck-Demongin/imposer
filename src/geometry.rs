pub const MM_PER_INCH: f32 = 25.4;
pub const PT_PER_INCH: f32 = 72.0;

pub fn mm_to_pt(mm: f32) -> f32 {
    mm * PT_PER_INCH / MM_PER_INCH
}

pub fn pt_to_mm(pt: f32) -> f32 {
    pt * MM_PER_INCH / PT_PER_INCH
}

pub fn mm_to_pt_f64(mm: f64) -> f64 {
    mm * (PT_PER_INCH as f64) / (MM_PER_INCH as f64)
}

pub fn pt_to_mm_f64(pt: f64) -> f64 {
    pt * (MM_PER_INCH as f64) / (PT_PER_INCH as f64)
}

#[derive(Debug, Clone, Copy)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}
