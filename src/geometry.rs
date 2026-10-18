pub const MM_PER_INCH: f32 = 25.4;
pub const PT_PER_INCH: f32 = 72.0;

pub fn mm_to_pt(mm: f32) -> f32 {
    mm * PT_PER_INCH / MM_PER_INCH
}

#[derive(Debug, Clone, Copy)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}
