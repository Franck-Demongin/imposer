use flate2::Compression;
use flate2::write::ZlibEncoder;
use imposer::SourceKind;
use imposer::SourcePage;
use imposer::psd_composite::parse_psd_composite;
use imposer::source::ImageColorSpace;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use tempfile::TempDir;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

static ORACLE_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

const ICC_ASSET: &str = "/home/franck/Dev/Rust/Imposer/assets/ISOcoated_v2_300_eci.icc";

fn external_command_available(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn minimal_icc_profile() -> Vec<u8> {
    if let Ok(bytes) = fs::read(ICC_ASSET)
        && !bytes.is_empty()
    {
        return bytes;
    }
    let mut b = vec![0u8; 128];
    b[0..4].copy_from_slice(&128u32.to_be_bytes());
    b[8] = 4;
    b[12..16].copy_from_slice(b"prtr");
    b[16..20].copy_from_slice(b"CMYK");
    b[20..24].copy_from_slice(b"Lab ");
    b[36..40].copy_from_slice(b"acsp");
    b
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
enum ModeSpec {
    Gray { channels: u16 },
    Rgb { channels: u16 },
    Cmyk { channels: u16 },
}

fn raw_interleaved(width: u32, height: u32, channels: u16, depth: u16, seed: u8) -> Vec<u8> {
    let pixels = (width * height) as usize;
    let bps = (depth / 8) as usize;
    let mut out = Vec::with_capacity(pixels * channels as usize * bps);
    for p in 0..pixels {
        for ch in 0..channels as usize {
            let v = ((seed as u32 * 97 + p as u32 * 31 + ch as u32 * 17) % 251) as u8;
            if depth == 8 {
                out.push(v);
            } else {
                out.extend_from_slice(&((v as u16 * 257).to_be_bytes()));
            }
        }
    }
    out
}

fn image_plane_inversion(value: u16, depth: u16) -> u16 {
    let max = if depth == 8 { 255u16 } else { 65535u16 };
    max - value
}

fn rle_encode_row(chunk: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < chunk.len() {
        let remaining = chunk.len() - pos;
        let len = remaining.min(128);
        if len == 1 {
            out.push(0);
            out.push(chunk[pos]);
        } else {
            out.push((len as u8) - 1);
            out.extend_from_slice(&chunk[pos..pos + len]);
        }
        pos += len;
    }
    out
}

#[expect(clippy::too_many_arguments)]
fn encode_psd(
    width: u32,
    height: u32,
    compression: u16,
    mode: ModeSpec,
    depth: u16,
    channels: u16,
    profile: Option<&[u8]>,
    interleaved: &[u8],
) -> Vec<u8> {
    let bps = (depth / 8) as usize;
    let pixels = (width * height) as usize;
    assert_eq!(interleaved.len(), pixels * channels as usize * bps);

    let color_mode = match mode {
        ModeSpec::Gray { .. } => 1u16,
        ModeSpec::Rgb { .. } => 3u16,
        ModeSpec::Cmyk { .. } => 4u16,
    };

    let mut b = Vec::new();
    b.extend_from_slice(b"8BPS");
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&[0u8; 6]);
    b.extend_from_slice(&channels.to_be_bytes());
    b.extend_from_slice(&height.to_be_bytes());
    b.extend_from_slice(&width.to_be_bytes());
    b.extend_from_slice(&depth.to_be_bytes());
    b.extend_from_slice(&color_mode.to_be_bytes());

    b.extend_from_slice(&0u32.to_be_bytes()); // color mode data

    let mut res = Vec::new();
    if let Some(profile) = profile {
        res.extend_from_slice(b"8BIM");
        res.extend_from_slice(&0x040fu16.to_be_bytes());
        res.push(0);
        res.push(0);
        res.extend_from_slice(&(profile.len() as u32).to_be_bytes());
        res.extend_from_slice(profile);
        if profile.len() % 2 == 1 {
            res.push(0);
        }
    }
    b.extend_from_slice(&(res.len() as u32).to_be_bytes());
    b.extend_from_slice(&res);

    b.extend_from_slice(&0u32.to_be_bytes()); // layer and mask info

    b.extend_from_slice(&compression.to_be_bytes());

    if compression == 0 {
        for ch in 0..channels as usize {
            for p in 0..pixels {
                for byte in 0..bps {
                    b.push(interleaved[p * channels as usize * bps + ch * bps + byte]);
                }
            }
        }
    } else if compression == 1 {
        let mut compressed_rows = Vec::new();
        for ch in 0..channels as usize {
            for row in 0..height as usize {
                let row_len = (width as usize) * bps;
                let mut row_pixels = Vec::with_capacity(row_len);
                for i in 0..row_len {
                    let idx = (row * (width as usize) + i / bps) * channels as usize * bps
                        + ch * bps
                        + (i % bps);
                    row_pixels.push(interleaved[idx]);
                }
                compressed_rows.push(rle_encode_row(&row_pixels));
            }
        }
        for payload in &compressed_rows {
            b.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        for payload in &compressed_rows {
            b.extend_from_slice(payload);
        }
    } else if compression == 2 || compression == 3 {
        let mut raw = Vec::with_capacity(pixels * channels as usize * bps);
        for ch in 0..channels as usize {
            for p in 0..pixels {
                for byte in 0..bps {
                    raw.push(interleaved[p * channels as usize * bps + ch * bps + byte]);
                }
            }
        }
        let mut payload = raw.clone();
        if compression == 3 {
            let plane_len = pixels * bps;
            for plane in 0..channels as usize {
                let plane_start = plane * plane_len;
                let mut diff = vec![0u8; plane_len];
                for row in 0..height as usize {
                    let row_start = row * (width as usize) * bps;
                    if bps == 1 {
                        for i in 0..(width as usize) {
                            let left = if i == 0 { 0 } else { diff[row_start + i - 1] };
                            diff[row_start + i] =
                                raw[plane_start + row_start + i].wrapping_sub(left);
                        }
                    } else {
                        for sample in 0..(width as usize) {
                            let off = row_start + sample * bps;
                            if sample == 0 {
                                diff[off] = raw[plane_start + off];
                                diff[off + 1] = raw[plane_start + off + 1];
                            } else {
                                let prev = u16::from_be_bytes([
                                    raw[plane_start + off - bps],
                                    raw[plane_start + off - bps + 1],
                                ]);
                                let current = u16::from_be_bytes([
                                    raw[plane_start + off],
                                    raw[plane_start + off + 1],
                                ]);
                                let out = current.wrapping_sub(prev);
                                diff[off..off + 2].copy_from_slice(&out.to_be_bytes());
                            }
                        }
                    }
                }
                payload[plane_start..plane_start + plane_len].copy_from_slice(&diff);
            }
        }
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&payload).unwrap();
        b.extend_from_slice(&enc.finish().unwrap());
    } else {
        panic!("unsupported compression");
    }
    b
}

fn layer_end(base: &[u8]) -> usize {
    // 26-byte header
    let mut pos = 26usize;
    let cmd_len =
        u32::from_be_bytes([base[pos], base[pos + 1], base[pos + 2], base[pos + 3]]) as usize;
    pos += 4 + cmd_len;
    let res_len =
        u32::from_be_bytes([base[pos], base[pos + 1], base[pos + 2], base[pos + 3]]) as usize;
    pos += 4 + res_len;
    let layer_len =
        u32::from_be_bytes([base[pos], base[pos + 1], base[pos + 2], base[pos + 3]]) as usize;
    pos += 4 + layer_len;
    pos
}

#[expect(clippy::too_many_arguments)]
fn layered_zip_psd(
    width: u32,
    height: u32,
    compression: u16,
    mode: ModeSpec,
    depth: u16,
    channels: u16,
    profile: Option<&[u8]>,
    interleaved: &[u8],
) -> Vec<u8> {
    let bps = (depth / 8) as usize;
    let pixels = (width as usize) * (height as usize);
    let use_profile = profile.is_some();
    let plain_channels = match mode {
        ModeSpec::Gray { .. } => 1usize,
        ModeSpec::Rgb { .. } => 3usize,
        ModeSpec::Cmyk { .. } => 4usize,
    };

    let tmp_dir: TempDir = tempfile::tempdir().unwrap();
    let tmp_stem = format!(
        "imposer-{}-{}",
        std::process::id(),
        ORACLE_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let icc_path = tmp_dir.path().join(format!("{tmp_stem}.icc"));
    let gray_path = tmp_dir.path().join(format!("{tmp_stem}.pgm"));
    let rgb_path = tmp_dir.path().join(format!("{tmp_stem}.ppm"));
    let cmyk_path = tmp_dir.path().join(format!("{tmp_stem}.cmyk"));
    let psd_path = tmp_dir.path().join(format!("{tmp_stem}.psd"));
    if use_profile {
        std::fs::write(&icc_path, profile.unwrap()).unwrap();
    }

    if matches!(mode, ModeSpec::Gray { .. }) {
        let mut raw = Vec::new();
        let maxval = if depth == 8 { 255u16 } else { 65535u16 };
        raw.extend_from_slice(format!("P5\n{} {}\n{}\n", width, height, maxval).as_bytes());
        for p in 0..pixels {
            let value: u16 = if depth == 8 {
                interleaved[p * channels as usize] as u16
            } else {
                u16::from_be_bytes([
                    interleaved[p * channels as usize * 2],
                    interleaved[p * channels as usize * 2 + 1],
                ])
            };
            if depth == 8 {
                raw.push(value as u8);
            } else {
                raw.extend_from_slice(&value.to_be_bytes());
            }
        }
        std::fs::write(&gray_path, &raw).unwrap();
    } else if matches!(mode, ModeSpec::Rgb { .. }) {
        let mut raw = Vec::new();
        let maxval = if depth == 8 { 255u16 } else { 65535u16 };
        raw.extend_from_slice(format!("P6\n{} {}\n{}\n", width, height, maxval).as_bytes());
        for p in 0..pixels {
            for ch in 0..3usize {
                let value: u16 = if depth == 8 {
                    interleaved[p * channels as usize + ch] as u16
                } else {
                    u16::from_be_bytes([
                        interleaved[p * channels as usize * 2 + ch * 2],
                        interleaved[p * channels as usize * 2 + ch * 2 + 1],
                    ])
                };
                if depth == 8 {
                    raw.push(value as u8);
                } else {
                    raw.extend_from_slice(&value.to_be_bytes());
                }
            }
        }
        std::fs::write(&rgb_path, &raw).unwrap();
    } else {
        let mut raw = Vec::with_capacity(pixels * 4 * bps);
        for p in 0..pixels {
            for ch in 0..4usize {
                let value = if depth == 8 {
                    interleaved[p * channels as usize + ch] as u16
                } else {
                    u16::from_be_bytes([
                        interleaved[p * channels as usize * 2 + ch * 2],
                        interleaved[p * channels as usize * 2 + ch * 2 + 1],
                    ])
                };
                if depth == 8 {
                    raw.push(value as u8);
                } else {
                    raw.extend_from_slice(&value.to_be_bytes());
                }
            }
        }
        std::fs::write(&cmyk_path, &raw).unwrap();
    }

    let mut cmd = Command::new("magick");
    if matches!(mode, ModeSpec::Gray { .. }) {
        cmd.arg(gray_path.clone()).arg(psd_path.clone());
    } else if matches!(mode, ModeSpec::Rgb { .. }) {
        cmd.arg(rgb_path.clone()).arg(psd_path.clone());
    } else {
        cmd.arg("-size")
            .arg(format!("{}x{}", width, height))
            .arg("-depth")
            .arg(depth.to_string())
            .arg(format!("CMYK:{}", cmyk_path.display()));
        if use_profile {
            cmd.arg("-profile").arg(icc_path.clone());
        }
        cmd.arg(psd_path.clone());
    }
    assert!(
        cmd.status().unwrap().success(),
        "ImageMagick base PSD failed"
    );
    let base = fs::read(&psd_path).unwrap();
    let prefix = &base[..layer_end(&base)];

    let mut raw = Vec::with_capacity(plain_channels * pixels * bps);
    for ch in 0..plain_channels {
        for p in 0..pixels {
            let value = if matches!(mode, ModeSpec::Cmyk { .. }) {
                image_plane_inversion(
                    if depth == 8 {
                        interleaved[p * channels as usize + ch] as u16
                    } else {
                        u16::from_be_bytes([
                            interleaved[p * channels as usize * 2 + ch * 2],
                            interleaved[p * channels as usize * 2 + ch * 2 + 1],
                        ])
                    },
                    depth,
                )
            } else {
                if depth == 8 {
                    interleaved[p * channels as usize + ch] as u16
                } else {
                    u16::from_be_bytes([
                        interleaved[p * channels as usize * 2 + ch * 2],
                        interleaved[p * channels as usize * 2 + ch * 2 + 1],
                    ])
                }
            };
            if depth == 8 {
                raw.push(value as u8);
            } else {
                raw.extend_from_slice(&value.to_be_bytes());
            }
        }
    }
    let mut payload = raw.clone();
    if compression == 3 {
        let plane_len = pixels * bps;
        for plane in 0..plain_channels {
            let plane_start = plane * plane_len;
            let mut diff = vec![0u8; plane_len];
            for row in 0..height as usize {
                let row_start = row * (width as usize) * bps;
                if bps == 1 {
                    for i in 0..(width as usize) {
                        let left = if i == 0 { 0 } else { diff[row_start + i - 1] };
                        diff[row_start + i] = raw[plane_start + row_start + i].wrapping_sub(left);
                    }
                } else {
                    for sample in 0..(width as usize) {
                        let off = row_start + sample * bps;
                        if sample == 0 {
                            diff[off] = raw[plane_start + off];
                            diff[off + 1] = raw[plane_start + off + 1];
                        } else {
                            let prev = u16::from_be_bytes([
                                raw[plane_start + off - bps],
                                raw[plane_start + off - bps + 1],
                            ]);
                            let current = u16::from_be_bytes([
                                raw[plane_start + off],
                                raw[plane_start + off + 1],
                            ]);
                            let out = current.wrapping_sub(prev);
                            diff[off..off + 2].copy_from_slice(&out.to_be_bytes());
                        }
                    }
                }
            }
            payload[plane_start..plane_start + plane_len].copy_from_slice(&diff);
        }
    }

    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&payload).unwrap();
    let zipped = enc.finish().unwrap();

    let mut out = Vec::new();
    out.extend_from_slice(prefix);
    out.extend_from_slice(&compression.to_be_bytes());
    out.extend_from_slice(&zipped);
    out
}

fn decode_magick_reference(path: &str) -> (u32, u32, Vec<u8>, bool, Option<Vec<u8>>) {
    let mut dec = Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
    let (w, h) = dec.dimensions().unwrap();
    let ct = dec.colortype().unwrap();
    let icc = dec.get_tag_u8_vec(Tag::IccProfile).ok();
    let pixels = dec.read_image().unwrap();
    let raw = match pixels {
        DecodingResult::U8(v) => v,
        DecodingResult::U16(v) => v.iter().map(|x| (*x >> 8) as u8).collect(),
        _ => panic!("unsupported decoded sample"),
    };
    let samples = match ct {
        tiff::ColorType::Gray(_) => 1,
        tiff::ColorType::Multiband { num_samples: n, .. } => n as usize,
        tiff::ColorType::RGB(_) => 3,
        tiff::ColorType::RGBA(_) => 4,
        tiff::ColorType::CMYK(_) => 4,
        tiff::ColorType::CMYKA(_) => 5,
        _ => panic!("unsupported TIFF color: {:?}", ct),
    };
    assert_eq!(raw.len(), w as usize * h as usize * samples);
    let mut pixels = Vec::with_capacity(w as usize * h as usize * 3);
    let is_cmyk = matches!(ct, tiff::ColorType::CMYK(_) | tiff::ColorType::CMYKA(_));
    for chunk in raw.chunks(samples as usize) {
        match ct {
            tiff::ColorType::Gray(_) | tiff::ColorType::GrayA(_) => {
                pixels.extend_from_slice(&[chunk[0], chunk[0], chunk[0]]);
            }
            tiff::ColorType::RGB(_) | tiff::ColorType::RGBA(_) => {
                pixels.extend_from_slice(&chunk[0..3]);
            }
            tiff::ColorType::CMYK(_) | tiff::ColorType::CMYKA(_) => {
                pixels.extend_from_slice(&chunk[0..4]);
            }
            tiff::ColorType::Multiband { .. } => {
                pixels.extend_from_slice(&[chunk[0], chunk[0], chunk[0]]);
            }
            _ => panic!(),
        }
    }
    (w, h, pixels, is_cmyk, icc)
}

fn sha_bytes(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

fn check_case(
    mode: ModeSpec,
    depth: u16,
    channels: u16,
    compression: u16,
    seed: u8,
    with_profile: bool,
) {
    let profile = if with_profile {
        Some(minimal_icc_profile())
    } else {
        None
    };
    let pass_profile = with_profile && matches!(mode, ModeSpec::Cmyk { .. });
    let dims = (3u32, 2u32);
    let inter = raw_interleaved(dims.0, dims.1, channels, depth, seed);

    let psd = if compression >= 2 {
        layered_zip_psd(
            dims.0,
            dims.1,
            compression,
            mode,
            depth,
            channels,
            if pass_profile {
                profile.as_deref()
            } else {
                None
            },
            &inter,
        )
    } else {
        encode_psd(
            dims.0,
            dims.1,
            compression,
            mode,
            depth,
            channels,
            if pass_profile {
                profile.as_deref()
            } else {
                None
            },
            &inter,
        )
    };

    let internal = parse_psd_composite(&psd).expect("internal decode failed");
    let sp = internal;
    let SourcePage {
        width_pt,
        height_pt,
        kind,
        ..
    } = sp;
    let (ipx, ipy, ips, icspace, picc) = match kind {
        SourceKind::Raster {
            width_px,
            height_px,
            pixels,
            color_space,
            icc_profile,
            ..
        } => (
            width_px,
            height_px,
            pixels.clone(),
            color_space,
            icc_profile.clone(),
        ),
        _ => panic!("internal not raster"),
    };

    let dir = std::env::temp_dir().join(format!(
        "imposer-psd-diff-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let psz = dir.join("in.psd");
    let tif = dir.join("out.tiff");
    fs::write(&psz, &psd).unwrap();
    let status = Command::new("magick")
        .arg(format!("{}[0]", psz.display()))
        .arg("-flatten")
        .arg("-depth")
        .arg("8")
        .arg("-compress")
        .arg("none")
        .arg(&tif)
        .status()
        .unwrap();
    assert!(status.success(), "ImageMagick failed");
    let (mw, mh, mp, mc, micc) = decode_magick_reference(tif.to_str().unwrap());
    assert_eq!((ipx, ipy), (mw, mh));
    assert_eq!(ips, mp);
    assert_eq!(icspace == ImageColorSpace::DeviceCmyk, mc);
    match (picc, micc) {
        (Some(a), Some(b)) => assert_eq!(sha_bytes(&a), sha_bytes(&b), "ICC mismatch"),
        (None, None) => {}
        (a, b) => panic!(
            "profile presence differs (internal {:?}, magick {:?})",
            a.map(|x| x.len()),
            b.map(|x| x.len())
        ),
    }

    let _ = (width_pt, height_pt, mc);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn differential_raw_gray8() {
    if !external_command_available("magick") {
        return;
    }
    for channels in [1u16, 2u16] {
        check_case(ModeSpec::Gray { channels }, 8, channels, 0, 1, false);
    }
}

#[test]
fn differential_raw_rgb8() {
    if !external_command_available("magick") {
        return;
    }
    for channels in [3u16, 4u16] {
        check_case(ModeSpec::Rgb { channels }, 8, channels, 0, 2, false);
    }
}

#[test]
fn differential_raw_cmyk8() {
    if !external_command_available("magick") {
        return;
    }
    for channels in [4u16, 5u16] {
        check_case(ModeSpec::Cmyk { channels }, 8, channels, 0, 3, true);
    }
}

#[test]
fn differential_rle_gray8() {
    if !external_command_available("magick") {
        return;
    }
    for channels in [1u16, 2u16] {
        check_case(ModeSpec::Gray { channels }, 8, channels, 1, 4, false);
    }
}

#[test]
fn differential_rle_rgb16() {
    if !external_command_available("magick") {
        return;
    }
    for channels in [3u16, 4u16] {
        check_case(ModeSpec::Rgb { channels }, 16, channels, 1, 5, false);
    }
}

#[test]
fn differential_zip_rgb8() {
    if !external_command_available("magick") {
        return;
    }
    for channels in [3u16, 4u16] {
        check_case(ModeSpec::Rgb { channels }, 8, channels, 2, 6, false);
    }
}

#[test]
fn differential_predictive_cmyk16() {
    if !external_command_available("magick") {
        return;
    }
    for channels in [4u16, 5u16] {
        check_case(ModeSpec::Cmyk { channels }, 16, channels, 3, 7, true);
    }
}
