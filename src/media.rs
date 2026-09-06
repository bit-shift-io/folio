//! Best-effort metadata extraction from file headers, dependency-free.
//! Covers what is cheap to read from the front of a file: image dimensions
//! and durations for the common audio/video containers (mp3, wav, flac,
//! mp4/m4a/mov). Anything else simply reports no media properties.

use std::fs;
use std::io::Read;
use std::path::Path;

use serde::Serialize;

/// Extra metadata for media files. All fields are optional; the presence of
/// whatever was extractable is all the client needs.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MediaInfo {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_secs: Option<f64>,
}

/// Bytes read from the file head; every parser below works on this window
/// except mp4, whose `moov` box often lives at the end of the file.
const HEAD_CAP: usize = 256 * 1024;
/// Files at or under this size are read wholesale for mp4 scanning; larger
/// ones get a head + tail scan for `moov`.
const MP4_FULL_CAP: u64 = 8 * 1024 * 1024;

/// Attempts to extract media properties for a path with the given MIME type.
/// Returns `None` when nothing is extractable (not media, or an unsupported
/// container).
pub fn probe(path: &Path, mime: &str) -> Option<MediaInfo> {
    if mime.starts_with("image/") {
        let head = read_head(path, HEAD_CAP)?;
        let (width, height) = image_dimensions(&head, mime)?;
        return Some(MediaInfo {
            width: Some(width),
            height: Some(height),
            duration_secs: None,
        });
    }
    let metadata = fs::metadata(path).ok()?;
    let size = metadata.len();
    let duration = match mime {
        "audio/x-wav" | "audio/wav" => read_head(path, HEAD_CAP)
            .and_then(|b| wav_duration(&b, size)),
        "audio/flac" | "audio/x-flac" => {
            read_head(path, HEAD_CAP).and_then(|b| flac_duration(&b))
        }
        "audio/mpeg" => read_head(path, HEAD_CAP).and_then(|b| mp3_duration(&b, size)),
        "audio/mp4" | "video/mp4" | "video/quicktime" | "audio/m4a" => mp4_duration(path),
        _ => None,
    }?;
    Some(MediaInfo {
        width: None,
        height: None,
        duration_secs: Some(duration),
    })
}

fn read_head(path: &Path, cap: usize) -> Option<Vec<u8>> {
    let size = fs::metadata(path).ok()?.len().min(cap as u64) as usize;
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size];
    fs::File::open(path).ok()?.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// Image dimensions from the container header; `mime` decides which format
/// to look at. Returns `None` when the header is too short or unsupported.
pub fn image_dimensions(buf: &[u8], mime: &str) -> Option<(u32, u32)> {
    if mime == "image/png" && buf.len() >= 24 && buf.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(buf, 16)?, be32(buf, 20)?));
    }
    if mime == "image/jpeg" && buf.len() >= 4 && buf[0] == 0xFF && buf[1] == 0xD8 {
        return jpeg_dimensions(buf);
    }
    if mime == "image/gif" && buf.len() >= 10
        && (buf.starts_with(b"GIF87a") || buf.starts_with(b"GIF89a"))
    {
        return Some((le16(buf, 6)? as u32, le16(buf, 8)? as u32));
    }
    if mime == "image/bmp" && buf.len() >= 26 && buf.starts_with(b"BM") {
        return Some((le32(buf, 18)?, le32(buf, 22)?));
    }
    if mime == "image/webp" {
        return webp_dimensions(buf);
    }
    None
}

fn jpeg_dimensions(buf: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 4 <= buf.len() {
        if buf[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = buf[i + 1];
        // Standalone / restart markers carry no length.
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        let len = be16(buf, i + 2)? as usize;
        // SOFn markers (not DHT/DAC/JPG) define height then width.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            return Some((be16(buf, i + 7)? as u32, be16(buf, i + 5)? as u32));
        }
        i += 2 + len;
    }
    None
}

fn webp_dimensions(buf: &[u8]) -> Option<(u32, u32)> {
    if buf.len() < 30 || !buf.starts_with(b"RIFF") || &buf[8..12] != b"WEBP" {
        return None;
    }
    match &buf[12..16] {
        // VP8X: canvas size minus one, little-endian 24-bit each.
        b"VP8X" if buf.len() >= 30 => {
            let width = le24(buf, 24)? + 1;
            let height = le24(buf, 27)? + 1;
            Some((width, height))
        }
        // VP8L (lossless): 14-bit dimensions packed in 4 bytes after the
        // signature byte.
        b"VP8L" if buf.len() >= 25 && buf[20] == 0x2F => {
            let bits = u32::from_le_bytes([buf[21], buf[22], buf[23], buf[24]]);
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        _ => None,
    }
}

/// Duration for a PCM WAV from its `fmt` byte rate and `data` chunk size.
fn wav_duration(buf: &[u8], size: u64) -> Option<f64> {
    if buf.len() < 12 || !buf.starts_with(b"RIFF") || &buf[8..12] != b"WAVE" {
        return None;
    }
    let mut byte_rate: Option<u64> = None;
    let mut data_size: Option<u64> = None;
    let mut i = 12;
    while i + 8 <= buf.len() {
        let id = &buf[i..i + 4];
        let len = le32(buf, i + 4)? as usize;
        let body = i + 8;
        match id {
            b"fmt " if len >= 16 => byte_rate = le32(buf, body + 8).map(|r| r as u64),
            b"data" if len > 0 || size > 0 => {
                data_size = Some(if len > 0 { len as u64 } else { size - 36 });
            }
            _ => {}
        }
        if byte_rate.is_some() && data_size.is_some() {
            break;
        }
        let step = 8 + len + (len % 2);
        if step == 0 || i + step > buf.len() {
            break;
        }
        i += step;
    }
    match (byte_rate, data_size) {
        (Some(rate), Some(data)) if rate > 0 => Some(data as f64 / rate as f64),
        _ => None,
    }
}

/// Duration for FLAC from the STREAMINFO block's total samples / sample rate.
fn flac_duration(buf: &[u8]) -> Option<f64> {
    if buf.len() < 42 || !buf.starts_with(b"fLaC") {
        return None;
    }
    let block_type = buf[4] & 0x7F;
    let block_len = be24(buf, 5)? as usize;
    if block_type != 0 || block_len < 34 {
        return None;
    }
    // STREAMINFO body begins at 8; its packed sample-rate/channels/bps/total
    // field sits at body offset 10..18, i.e. absolute 18..26.
    let s = &buf[18..26];
    let packed = u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]);
    let sample_rate = ((packed >> 44) & 0xFFFFF) as u32;
    let total = packed & 0xF_FFFF_FFFF;
    if sample_rate == 0 {
        return None;
    }
    Some(total as f64 / sample_rate as f64)
}

const MPEG1_L3_BITRATE_K: [u32; 16] =
    [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0];

/// Duration estimate for CBR MP3: `size * 8 / bitrate`, from the first MPEG
/// audio frame after any ID3v2 tag.
fn mp3_duration(buf: &[u8], size: u64) -> Option<f64> {
    let mut start = 0usize;
    if buf.len() >= 10 && buf.starts_with(b"ID3") {
        start = 10
            + ((buf[6] as usize) << 21)
            + ((buf[7] as usize) << 14)
            + ((buf[8] as usize) << 7)
            + buf[9] as usize;
    }
    let mut i = start;
    while i + 4 <= buf.len() {
        if buf[i] == 0xFF && buf[i + 1] & 0xE0 == 0xE0 {
            let version = (buf[i + 1] >> 3) & 0x3;
            let layer = (buf[i + 1] >> 1) & 0x3;
            if version == 3 && layer == 1 {
                let kbps = MPEG1_L3_BITRATE_K[(buf[i + 2] >> 4) as usize];
                if kbps > 0 {
                    return Some(size as f64 * 8.0 / (kbps as f64 * 1000.0));
                }
            }
            return None;
        }
        i += 1;
    }
    None
}

/// Duration for iso-bmff containers (mp4/m4a/mov) from the `mvhd` box.
fn mp4_duration(path: &Path) -> Option<f64> {
    let size = fs::metadata(path).ok()?.len();
    let bytes = if size <= MP4_FULL_CAP {
        read_head(path, MP4_FULL_CAP as usize)?
    } else {
        let mut buf = read_head(path, 1024 * 1024)?;
        let tail_len = (4 * 1024 * 1024).min(size as usize);
        let mut file = fs::File::open(path).ok()?;
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::End(-(tail_len as i64))).ok()?;
        let mut tail = vec![0u8; tail_len];
        file.read_exact(&mut tail).ok()?;
        buf.extend_from_slice(&tail);
        buf
    };
    find_mvhd_duration(&bytes)
}

fn find_mvhd_duration(bytes: &[u8]) -> Option<f64> {
    let idx = bytes.windows(4).position(|w| w == b"mvhd")?;
    // `idx` points at the box type; the payload begins right after it.
    let payload = idx + 4;
    let version = *bytes.get(payload)?;
    let (ts_off, duration_off, duration_len) = if version == 0 {
        (payload + 12, payload + 16, 4)
    } else {
        (payload + 20, payload + 24, 8)
    };
    let timescale = be32(bytes, ts_off)? as f64;
    let duration = match duration_len {
        4 => be32(bytes, duration_off)? as f64,
        _ => be64(bytes, duration_off)? as f64,
    };
    if timescale <= 0.0 {
        return None;
    }
    Some(duration / timescale)
}

fn be16(buf: &[u8], off: usize) -> Option<u16> {
    let s = buf.get(off..off + 2)?;
    Some(((s[0] as u16) << 8) | s[1] as u16)
}

fn be24(buf: &[u8], off: usize) -> Option<u32> {
    let s = buf.get(off..off + 3)?;
    Some(((s[0] as u32) << 16) | ((s[1] as u32) << 8) | s[2] as u32)
}

fn be32(buf: &[u8], off: usize) -> Option<u32> {
    let s = buf.get(off..off + 4)?;
    Some(((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | s[3] as u32)
}

fn be64(buf: &[u8], off: usize) -> Option<u64> {
    let s = buf.get(off..off + 8)?;
    Some(u64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

fn le16(buf: &[u8], off: usize) -> Option<u16> {
    let s = buf.get(off..off + 2)?;
    Some((s[0] as u16) | ((s[1] as u16) << 8))
}

fn le24(buf: &[u8], off: usize) -> Option<u32> {
    let s = buf.get(off..off + 3)?;
    Some((s[0] as u32) | ((s[1] as u32) << 8) | ((s[2] as u32) << 16))
}

fn le32(buf: &[u8], off: usize) -> Option<u32> {
    let s = buf.get(off..off + 4)?;
    Some((s[0] as u32) | ((s[1] as u32) << 8) | ((s[2] as u32) << 16) | ((s[3] as u32) << 24))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_dimensions_from_ihdr() {
        let mut buf = vec![0u8; 24];
        buf[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        buf[16..20].copy_from_slice(&[0, 0, 0x07, 0x80]); // 1920
        buf[20..24].copy_from_slice(&[0, 0, 0x04, 0x38]); // 1080
        assert_eq!(image_dimensions(&buf, "image/png"), Some((1920, 1080)));
    }

    #[test]
    fn jpeg_dimensions_scan_markers() {
        let mut buf = vec![0u8; 0];
        buf.extend_from_slice(&[0xFF, 0xD8]); // SOI
        buf.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]); // APP0, 16 bytes
        buf.extend_from_slice(&[0u8; 16]);
        buf.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11]); // SOF0, 17 bytes
        buf.extend_from_slice(&[0x08, 0x02, 0xD0, 0x05, 0x00]); // 8-bit, h=720, w=1280
        assert_eq!(image_dimensions(&buf, "image/jpeg"), Some((1280, 720)));
    }

    #[test]
    fn gif_and_bmp_dimensions() {
        let gif = &b"GIF89a"[..].iter().copied().chain([0x14, 0x00, 0x0A, 0x00]).collect::<Vec<u8>>();
        assert_eq!(image_dimensions(gif, "image/gif"), Some((20, 10)));
        let mut bmp = vec![0u8; 26];
        bmp[..2].copy_from_slice(b"BM");
        bmp[18..22].copy_from_slice(&[0x40, 0x01, 0, 0]); // 320
        bmp[22..26].copy_from_slice(&[0x00, 0x01, 0, 0]); // 256
        assert_eq!(image_dimensions(&bmp, "image/bmp"), Some((320, 256)));
    }

    #[test]
    fn webp_vp8x_dimensions() {
        let mut buf = vec![0u8; 30];
        buf[..4].copy_from_slice(b"RIFF");
        buf[8..12].copy_from_slice(b"WEBP");
        buf[12..16].copy_from_slice(b"VP8X");
        buf[24..27].copy_from_slice(&[0xFF, 0x07, 0x00]); // width-1 = 2047
        buf[27..30].copy_from_slice(&[0x83, 0x00, 0x00]); // height-1 = 131
        assert_eq!(image_dimensions(&buf, "image/webp"), Some((2048, 132)));
    }

    #[test]
    fn wav_duration_from_fmt_and_data() {
        let mut buf = vec![0u8; 0];
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&[0, 0, 0x14, 0]); // size (unused)
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&[0x10, 0, 0, 0]); // 16-byte fmt body
        buf.extend_from_slice(&[1, 0, 1, 0]); // PCM, mono
        buf.extend_from_slice(&[0x44, 0xAC, 0, 0]); // 44100 Hz
        buf.extend_from_slice(&[0x00, 0x71, 0x02, 0]); // 160000 byte rate
        buf.extend_from_slice(&[2, 0, 0x10, 0]); // align, bits
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&[0x00, 0x40, 0x1F, 0]); // 2048000 bytes = 12.8s
        assert_eq!(wav_duration(&buf, buf.len() as u64), Some(12.8));
    }

    #[test]
    fn flac_duration_from_streaminfo() {
        let mut buf = vec![0u8; 42];
        buf[..4].copy_from_slice(b"fLaC");
        buf[4] = 0x80 | 0; // last metadata block, type STREAMINFO
        buf[5] = 0;
        buf[6] = 0;
        buf[7] = 34; // 34-byte STREAMINFO body
        let packed = (44100u64 << 44) | (1 << 41) | (2 << 36) | 132300u64;
        buf[18..26].copy_from_slice(&packed.to_be_bytes());
        assert_eq!(flac_duration(&buf), Some(3.0));
    }

    #[test]
    fn mp3_duration_estimates_from_cbr_bitrate() {
        let mut buf = vec![0u8; 0];
        buf.extend_from_slice(b"ID3");
        buf.extend_from_slice(&[0x03, 0x00, 0x00]);
        buf.extend_from_slice(&[0, 0, 0, 0]); // synchsafe size 0
        buf.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]); // MPEG1 L3, 128kbps
        assert_eq!(mp3_duration(&buf, 16 * 1000), Some(1.0));
    }

    #[test]
    fn mvhd_duration_version0() {
        let mut buf = vec![0u8; 0];
        buf.extend_from_slice(&[0, 0, 0, 20]); // box size
        buf.extend_from_slice(b"mvhd");
        buf.extend_from_slice(&[0, 0, 0, 0]); // version 0 + flags
        buf.extend_from_slice(&[0, 0, 0, 1]); // creation
        buf.extend_from_slice(&[0, 0, 0, 1]); // modification
        buf.extend_from_slice(&[0, 0, 0x3E, 0x80]); // timescale 16000
        buf.extend_from_slice(&[0, 0, 0x9D, 0x40]); // duration 40256 = 2.516s
        assert_eq!(find_mvhd_duration(&buf), Some(40256.0 / 16000.0));
    }

    #[test]
    fn image_dimensions_rejects_truncated_headers() {
        assert_eq!(image_dimensions(b"\x89PNG", "image/png"), None);
        assert_eq!(image_dimensions(b"GIF98a........", "image/gif"), None);
        assert_eq!(image_dimensions(b"nodatahere", "image/jpeg"), None);
    }
}