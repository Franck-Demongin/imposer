//! Internal decoder for Photoshop PSD/PSB **composite** image data.
//!
//! This module removes the runtime dependency on `magick` by reading the
//! final `Image Data` section directly. It reproduces the 8-bit interleaved
//! buffer consumed by `crate::source::make_psd_source_page`, preserving CMYK
//! inversion and alpha skipping behaviour observed from ImageMagick's TIFF
//! export.

use crate::error::{ImposerError, Result};
use crate::icc::IccProfile;
use crate::source::{self, ImageColorSpace};
use flate2::read::ZlibDecoder;
use std::io::Read;

const MAX_DECODED_BYTES: u64 = 1 << 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupportedMode {
    Gray,
    Rgb,
    Cmyk,
}

struct PsdHeader {
    version: u8,
    channels: u16,
    height: u32,
    width: u32,
    depth: u16,
    color_mode: u16,
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| ImposerError::Parse("PSD tronqué".to_string()))?;
        let s = self
            .buf
            .get(self.pos..end)
            .ok_or_else(|| ImposerError::Parse("PSD tronqué".to_string()))?;
        self.pos = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

fn unsupported(msg: String) -> ImposerError {
    ImposerError::PsdUnsupported(msg)
}

fn checked_plane_len(w: u32, h: u32, channels: u16, bytes_per_sample: usize) -> Result<usize> {
    let total = (w as u64)
        .saturating_mul(h as u64)
        .saturating_mul(channels as u64)
        .saturating_mul(bytes_per_sample as u64);
    if total > MAX_DECODED_BYTES {
        return Err(unsupported("PSD : données trop volumineuses".to_string()));
    }
    usize::try_from(total).map_err(|_| unsupported("PSD : données trop volumineuses".to_string()))
}

fn read_header(bytes: &[u8]) -> Result<(PsdHeader, usize)> {
    let mut cur = Cursor::new(bytes);
    if cur.take(4)? != b"8BPS" {
        return Err(ImposerError::Parse("PSD : signature invalide".to_string()));
    }
    let version_raw = cur.u16()?;
    let version = match version_raw {
        1 => 1u8,
        2 => 2u8,
        _ => return Err(unsupported(format!("version PSD {version_raw}"))),
    };
    cur.take(6)?;
    let channels = cur.u16()?;
    let height = cur.u32()?;
    let width = cur.u32()?;
    let depth = cur.u16()?;
    let color_mode = cur.u16()?;
    if width == 0 || height == 0 || channels == 0 {
        return Err(unsupported("PSD : dimensions ou canaux nuls".to_string()));
    }
    if depth != 8 && depth != 16 {
        return Err(unsupported(format!("profondeur PSD {depth}")));
    }
    match color_mode {
        1 | 3 | 4 => {}
        _ => return Err(unsupported(format!("mode PSD {color_mode}"))),
    }
    Ok((
        PsdHeader {
            version,
            channels,
            height,
            width,
            depth,
            color_mode,
        },
        cur.pos,
    ))
}

fn read_sections(bytes: &[u8], header: &PsdHeader, mut pos: usize) -> Result<usize> {
    // color mode data
    if pos + 4 > bytes.len() {
        return Err(ImposerError::Parse("PSD tronqué".to_string()));
    }
    let cmd_len =
        u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
    pos += 4;
    pos = pos
        .checked_add(cmd_len)
        .ok_or_else(|| ImposerError::Parse("PSD tronqué".to_string()))?;
    if pos > bytes.len() {
        return Err(ImposerError::Parse("PSD tronqué".to_string()));
    }

    // image resources
    if pos + 4 > bytes.len() {
        return Err(ImposerError::Parse("PSD tronqué".to_string()));
    }
    let res_len =
        u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]]) as usize;
    pos += 4;
    pos = pos
        .checked_add(res_len)
        .ok_or_else(|| ImposerError::Parse("PSD tronqué".to_string()))?;
    if pos > bytes.len() {
        return Err(ImposerError::Parse("PSD tronqué".to_string()));
    }

    // layer & mask
    if header.version == 2 {
        if pos + 8 > bytes.len() {
            return Err(ImposerError::Parse("PSD tronqué".to_string()));
        }
        let mut tmp = [0u8; 8];
        tmp.copy_from_slice(&bytes[pos..pos + 8]);
        let layer_len = usize::try_from(u64::from_be_bytes(tmp))
            .map_err(|_| ImposerError::Parse("PSD tronqué".to_string()))?;
        pos += 8;
        pos = pos
            .checked_add(layer_len)
            .ok_or_else(|| ImposerError::Parse("PSD tronqué".to_string()))?;
    } else {
        if pos + 4 > bytes.len() {
            return Err(ImposerError::Parse("PSD tronqué".to_string()));
        }
        let layer_len =
            u32::from_be_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
                as usize;
        pos += 4;
        pos = pos
            .checked_add(layer_len)
            .ok_or_else(|| ImposerError::Parse("PSD tronqué".to_string()))?;
    }
    if pos > bytes.len() {
        return Err(ImposerError::Parse("PSD tronqué".to_string()));
    }
    Ok(pos)
}

fn supported_mode(header: &PsdHeader) -> Result<SupportedMode> {
    match header.color_mode {
        1 => Ok(SupportedMode::Gray),
        3 => Ok(SupportedMode::Rgb),
        4 => Ok(SupportedMode::Cmyk),
        _ => Err(unsupported(format!("mode PSD {}", header.color_mode))),
    }
}

fn decode_raw(tail: &[u8], channels: usize, plane_len: usize) -> Result<Vec<Vec<u8>>> {
    let min_len = channels.saturating_mul(plane_len);
    if tail.len() < min_len {
        return Err(ImposerError::Parse("PSD : données incomplètes".to_string()));
    }
    let mut planes = Vec::with_capacity(channels);
    for c in 0..channels {
        planes.push(tail[c * plane_len..(c + 1) * plane_len].to_vec());
    }
    Ok(planes)
}

fn rle_decode_row(input: &[u8], expected: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(expected);
    let mut i = 0usize;
    while out.len() < expected {
        let n = *input
            .get(i)
            .ok_or_else(|| ImposerError::Parse("PSD : RLE tronqué".to_string()))?
            as i8;
        i += 1;
        if n >= 0 {
            let l = (n as usize) + 1;
            let src = input
                .get(i..i + l)
                .ok_or_else(|| ImposerError::Parse("PSD : RLE tronqué".to_string()))?;
            out.extend_from_slice(src);
            i += l;
        } else if n != -128 {
            let l = (1 - (n as i16)) as usize;
            let v = *input
                .get(i)
                .ok_or_else(|| ImposerError::Parse("PSD : RLE tronqué".to_string()))?;
            i += 1;
            out.extend(std::iter::repeat_n(v, l));
        }
    }
    if out.len() != expected {
        return Err(ImposerError::Parse(
            "PSD : RLE longueur inattendue".to_string(),
        ));
    }
    Ok(out)
}

fn decode_rle(
    header: &PsdHeader,
    tail: &[u8],
    plane_len: usize,
    row_len: usize,
    channels: usize,
    height: usize,
) -> Result<Vec<Vec<u8>>> {
    let psd = header.version == 1;
    let entry_size = if psd { 2usize } else { 4usize };
    let (height_u32, channels_u16) = (height, channels);
    let table_entries = channels_u16 * height_u32;
    let table_len = table_entries * entry_size;
    if tail.len() < table_len {
        return Err(ImposerError::Parse(
            "PSD : tables RLE tronquées".to_string(),
        ));
    }
    let mut counts = Vec::with_capacity(table_entries);
    let mut pos = 0usize;
    for _ in 0..table_entries {
        let n = if psd {
            usize::from(u16::from_be_bytes([tail[pos], tail[pos + 1]]))
        } else {
            usize::try_from(u32::from_be_bytes([
                tail[pos],
                tail[pos + 1],
                tail[pos + 2],
                tail[pos + 3],
            ]))
            .map_err(|_| unsupported("PSD".to_string()))?
        };
        counts.push(n);
        pos += entry_size;
    }
    let data = &tail[pos..];
    let mut src_idx = 0usize;
    let mut planes = Vec::with_capacity(channels);
    for c in 0..channels {
        let mut plane = Vec::with_capacity(plane_len);
        for r in 0..height_u32 {
            let idx = c * height_u32 + r;
            let n = counts[idx];
            if src_idx + n > data.len() {
                return Err(ImposerError::Parse("PSD : RLE tronqué".to_string()));
            }
            let row = rle_decode_row(&data[src_idx..src_idx + n], row_len)?;
            plane.extend_from_slice(&row);
            src_idx += n;
        }
        if plane.len() != plane_len {
            return Err(ImposerError::Parse(
                "PSD : plan RLE inconsistent".to_string(),
            ));
        }
        planes.push(plane);
    }
    let _ = plane_len;
    Ok(planes)
}

#[allow(clippy::too_many_arguments)]
fn decode_zip(
    header: &PsdHeader,
    tail: &[u8],
    plane_len: usize,
    row_len: usize,
    channels: usize,
    predict: bool,
    width: u32,
    bps: usize,
) -> Result<Vec<Vec<u8>>> {
    let _ = header;
    let _ = row_len;
    let total_len = channels.saturating_mul(plane_len);

    // Some exporters compress each planar channel in its own zlib stream.
    // This is compatible with subsequent streams because deflate streams are
    // self-delimiting; consume them until every planar channel is decoded.
    let mut cursor = std::io::Cursor::new(tail);
    let mut merged = Vec::with_capacity(total_len);
    let mut merged_decoder = ZlibDecoder::new(&mut cursor);
    merged_decoder
        .read_to_end(&mut merged)
        .map_err(|e| ImposerError::Parse(format!("PSD : ZIP {e}")))?;
    if cursor.position() == tail.len() as u64 && merged.len() == total_len {
        // Compatibility path: all channels in one zlib stream encourage.
        if predict {
            for ch in 0..channels {
                let start = ch * plane_len;
                apply_prediction(&mut merged[start..start + plane_len], width, bps);
            }
        }
        let mut planes = Vec::with_capacity(channels);
        for ch in 0..channels {
            let start = ch * plane_len;
            planes.push(merged[start..start + plane_len].to_vec());
        }
        return Ok(planes);
    }

    let mut planes = Vec::with_capacity(channels);
    let mut consumed = 0usize;
    for ch in 0..channels {
        if consumed >= tail.len() {
            return Err(ImposerError::Parse(
                "PSD : ZIP données incomplètes".to_string(),
            ));
        }
        let mut stream = flate2::Decompress::new(true);
        let mut plane = Vec::with_capacity(plane_len);
        loop {
            if plane.len() >= plane_len {
                return Err(ImposerError::Parse("PSD : ZIP canal trop gros".to_string()));
            }
            let mut output = vec![0u8; plane_len - plane.len()];
            let input = &tail[consumed..];
            let status = stream
                .decompress(input, &mut output, flate2::FlushDecompress::None)
                .map_err(|e| ImposerError::Parse(format!("PSD : ZIP {e}")))?;
            let received = stream.total_out() as usize - plane.len();
            plane.extend_from_slice(&output[..received]);
            let stream_consumed = stream.total_in() as usize;
            if plane.len() == plane_len {
                if status != flate2::Status::StreamEnd {
                    return Err(ImposerError::Parse("PSD : ZIP canal incomplet".to_string()));
                }
                consumed += stream_consumed;
                break;
            }
            if status == flate2::Status::StreamEnd {
                return Err(ImposerError::Parse(format!(
                    "PSD : décompression ZIP canal {} taille {} != {}",
                    ch,
                    plane.len(),
                    plane_len
                )));
            }
            if stream_consumed == 0 {
                return Err(ImposerError::Parse("PSD : ZIP flux nul".to_string()));
            }
        }
        if predict {
            apply_prediction(&mut plane, width, bps);
        }
        planes.push(plane);
    }
    if consumed != tail.len() {
        return Err(ImposerError::Parse(format!(
            "PSD : ZIP données excédentaires ({} octets inexpliqués)",
            tail.len() - consumed
        )));
    }
    Ok(planes)
}

pub fn parse_psd_composite(bytes: &[u8]) -> Result<source::SourcePage> {
    let (header, hdr_end) = read_header(bytes)?;
    let mode = supported_mode(&header)?;
    let img_pos = read_sections(bytes, &header, hdr_end)?;
    if img_pos + 2 > bytes.len() {
        return Err(ImposerError::Parse("PSD : pas d'image".to_string()));
    }
    let compression = u16::from_be_bytes([bytes[img_pos], bytes[img_pos + 1]]);
    let tail_start = img_pos + 2;
    let tail = &bytes[tail_start..];

    match compression {
        0..=3 => {}
        _ => return Err(unsupported(format!("compression PSD {compression}"))),
    }

    let channels = header.channels as usize;
    let needed_user = match mode {
        SupportedMode::Gray => 1,
        SupportedMode::Rgb => 3,
        SupportedMode::Cmyk => 4,
    };
    if channels < needed_user {
        return Err(unsupported("PSD : canaux insuffisants".to_string()));
    }

    let w = header.width;
    let h = header.height;
    let bps = (header.depth / 8) as usize;
    let plane_len = checked_plane_len(w, h, 1, bps)?;
    let plane_len_one = plane_len;
    if bps != 1 && bps != 2 {
        return Err(unsupported("PSD : échantillons".to_string()));
    }

    let all_planes = match compression {
        0 => decode_raw(tail, channels, plane_len_one)?,
        1 => decode_rle(
            &header,
            tail,
            plane_len_one,
            w as usize * bps,
            channels,
            h as usize,
        )?,
        2 => decode_zip(
            &header,
            tail,
            plane_len_one,
            w as usize * bps,
            channels,
            false,
            w,
            bps,
        )?,
        3 => decode_zip(
            &header,
            tail,
            plane_len_one,
            w as usize * bps,
            channels,
            true,
            w,
            bps,
        )?,
        _ => unreachable!(),
    };

    // collect needed high-byte 8-bit output.
    let pixels_count = (w as usize) * (h as usize);
    let mut out = Vec::with_capacity(pixels_count * needed_user);
    for p in 0..pixels_count {
        let mut sample = Vec::with_capacity(needed_user);
        for plane in &all_planes[..needed_user] {
            let hi = plane[p * bps];
            sample.push(hi);
        }
        match mode {
            SupportedMode::Gray => {
                out.push(sample[0]);
                out.push(sample[0]);
                out.push(sample[0]);
            }
            SupportedMode::Rgb => {
                out.push(sample[0]);
                out.push(sample[1]);
                out.push(sample[2]);
            }
            SupportedMode::Cmyk => {
                for v in sample {
                    out.push(255u8.wrapping_sub(v));
                }
            }
        }
    }

    let dpi = source::extract_psd_resolution(bytes).unwrap_or((300.0, 300.0));
    let color_space = match mode {
        SupportedMode::Gray | SupportedMode::Rgb => ImageColorSpace::DeviceRgb,
        SupportedMode::Cmyk => ImageColorSpace::DeviceCmyk,
    };
    let icc_profile = if matches!(mode, SupportedMode::Cmyk) {
        match source::extract_psd_embedded_profile(bytes) {
            Some(profile) => {
                IccProfile::parse(&profile).map_err(|e| ImposerError::InvalidIcc(e.to_string()))?;
                Some(profile)
            }
            // A generator that cannot apply its ICC profile may leave this
            // optional embedded resource absent. Pixel decoding still succeeds;
            // source::parse_psd enforces the policy requirement afterwards.
            None => None,
        }
    } else {
        None
    };

    source::make_psd_source_page(w, h, out, color_space, dpi, icc_profile)
}

fn apply_prediction(data: &mut [u8], width: u32, bps: usize) {
    let row_bytes = (width as usize) * bps;
    if row_bytes == 0 || bps == 0 || bps > 2 {
        return;
    }
    let rows = data.len() / row_bytes;
    if bps == 1 {
        for row in 0..rows {
            let row_start = row * row_bytes;
            let row_end = row_start + row_bytes;
            for i in row_start + 1..row_end {
                data[i] = data[i].wrapping_add(data[i - 1]);
            }
        }
    } else {
        for row in 0..rows {
            let row_start = row * row_bytes;
            for sample in 1..(width as usize) {
                let off = row_start + sample * bps;
                let a = u16::from_be_bytes([data[off], data[off + 1]]);
                let b = u16::from_be_bytes([
                    data[row_start + (sample - 1) * bps],
                    data[row_start + (sample - 1) * bps + 1],
                ]);
                let out = a.wrapping_add(b);
                data[off..off + 2].copy_from_slice(&out.to_be_bytes());
            }
        }
    }
}
