//! Save native-resolution 400x240 stereoscopic MPO photos to the 3DS Camera album.
//! The caller provides pristine BGR8 scene buffers (no on-screen notifications).
//! Save the Camera pair and individual native-resolution eye JPEGs only.
//! No changes to the emulator ROM, save format, or live render buffers.
//!
//! MPF APP2 + Exif DateTime layout follows the format used by the open-source
//! 3DS homebrew Comet (MIT): https://github.com/verypedro/Comet

use jpeg_encoder::{ColorType, Encoder};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const SCREEN_WIDTH: usize = 400;
const SCREEN_HEIGHT: usize = 240;
const CAMERA_FOLDER: &str = "sdmc:/DCIM/100NIN03";
const NATIVE_FOLDER: &str = "sdmc:/3ds/tg18-3ds/photos";

/// Native pixel-for-pixel top-eye photo: 400x240, no camera padding or resize.
/// Same pixel-for-pixel 400x240 eye view used in the new camera MPO.
fn encode_native_jpeg(frame: &[u8]) -> Result<Vec<u8>, String> {
    if frame.len() != SCREEN_WIDTH * SCREEN_HEIGHT * 3 {
        return Err("Unexpected native screenshot size".into());
    }
    let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
    for y in 0..SCREEN_HEIGHT {
        for x in 0..SCREEN_WIDTH {
            let src = (x * SCREEN_HEIGHT + (SCREEN_HEIGHT - 1 - y)) * 3;
            let dst = (y * SCREEN_WIDTH + x) * 3;
            rgb[dst] = frame[src + 2];
            rgb[dst + 1] = frame[src + 1];
            rgb[dst + 2] = frame[src];
        }
    }
    let mut output = Vec::new();
    Encoder::new(&mut output, 93)
        .encode(
            &rgb,
            SCREEN_WIDTH as u16,
            SCREEN_HEIGHT as u16,
            ColorType::Rgb,
        )
        .map_err(|e| format!("Native JPEG encoding failed: {e}"))?;
    Ok(output)
}

// Civil calendar from Unix days (Gregorian). No new chrono dependency.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}

fn exif_time() -> [u8; 20] {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let day = secs.div_euclid(86_400);
    let time = secs.rem_euclid(86_400);
    let (year, month, date) = civil_from_days(day);
    let s = format!(
        "{year:04}:{month:02}:{date:02} {:02}:{:02}:{:02}",
        time / 3600,
        (time / 60) % 60,
        time % 60
    );
    let mut buf = [0u8; 20];
    if s.len() == 19 {
        buf[..19].copy_from_slice(s.as_bytes());
    }
    buf
}

// 56 bytes including the FF E1 marker and length, with a valid Exif DateTime.
fn make_exif_app1() -> Vec<u8> {
    let mut exif = vec![0xFF, 0xE1, 0x00, 0x36];
    exif.extend_from_slice(b"Exif\0\0");
    exif.extend_from_slice(b"II\x2A\x00\x08\x00\x00\x00");
    exif.extend_from_slice(&1u16.to_le_bytes()); // 1 IFD entry
    exif.extend_from_slice(&0x0132u16.to_le_bytes()); // DateTime
    exif.extend_from_slice(&2u16.to_le_bytes()); // ASCII
    exif.extend_from_slice(&20u32.to_le_bytes());
    exif.extend_from_slice(&26u32.to_le_bytes());
    exif.extend_from_slice(&0u32.to_le_bytes()); // next IFD
    exif.extend_from_slice(&exif_time());
    debug_assert_eq!(exif.len(), 56);
    exif
}

// CIPA DC-007, two image disparity MPO, validated sizes and byte offsets.
// The primary JPEG contains this 90-byte APP2 segment; the second JPEG
// begins immediately after the primary EOI marker.
fn make_mpf_app2(left_len: u32, right_len: u32, right_offset: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(90);
    out.extend_from_slice(&[0xFF, 0xE2, 0x00, 0x58]); // APP2 length 88
    out.extend_from_slice(b"MPF\0");
    out.extend_from_slice(b"II\x2A\x00\x08\x00\x00\x00");
    out.extend_from_slice(&3u16.to_le_bytes());
    // MPFVersion, UNDEFINED, 4 bytes, value '0100'.
    out.extend_from_slice(&0xB000u16.to_le_bytes());
    out.extend_from_slice(&7u16.to_le_bytes());
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(b"0100");
    // NumberOfImages, LONG, 1 element.
    out.extend_from_slice(&0xB001u16.to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    // MPEntry, UNDEFINED, 32 bytes, TIFF offset 50.
    out.extend_from_slice(&0xB002u16.to_le_bytes());
    out.extend_from_slice(&7u16.to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&50u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // next IFD offset
                                                // First: representative multi-frame disparity JPEG.
    out.extend_from_slice(&0x2002_0002u32.to_le_bytes());
    out.extend_from_slice(&left_len.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // primary offset is zero
    out.extend_from_slice(&0u32.to_le_bytes()); // two dependent-image ids
                                                // Second: accompanying right-eye disparity JPEG.
    out.extend_from_slice(&0x0002_0002u32.to_le_bytes());
    out.extend_from_slice(&right_len.to_le_bytes());
    out.extend_from_slice(&right_offset.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    debug_assert_eq!(out.len(), 90);
    out
}

// Retain the encoder's APP0/JFIF segment (already emitted by jpeg-encoder).
fn insert_after_soi(jpeg: &[u8], segments: &[u8]) -> Result<Vec<u8>, String> {
    if !jpeg.starts_with(&[0xFF, 0xD8]) {
        return Err("Missing JPEG SOI".into());
    }
    let mut out = Vec::with_capacity(jpeg.len() + segments.len());
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(segments);
    out.extend_from_slice(&jpeg[2..]);
    Ok(out)
}

/// Build a native 400x240 stereo MPO and its matching 2D Camera JPEG.
fn build_photo(left_jpeg: &[u8], right_jpeg: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    if !right_jpeg.starts_with(&[0xFF, 0xD8]) {
        return Err("Missing right JPEG SOI".into());
    }
    let exif = make_exif_app1();
    // The MPF TIFF base is at offset 2(SOI) + 56(EXIF) + 8(APP2 header).
    let left_final_len = left_jpeg.len() + exif.len() + 90;
    let offset_from_mpf = left_final_len
        .checked_sub(2 + exif.len() + 8)
        .ok_or("Invalid MPO offsets")?;
    let app2 = make_mpf_app2(
        left_final_len as u32,
        right_jpeg.len() as u32,
        offset_from_mpf as u32,
    );
    let mut metadata = exif.clone();
    metadata.extend_from_slice(&app2);
    let first = insert_after_soi(&left_jpeg, &metadata)?;
    // Standard 3DS Camera stores HNI_XXXX.JPG beside HNI_XXXX.MPO.
    let jpg = insert_after_soi(&left_jpeg, &exif)?;
    let mut mpo = Vec::with_capacity(first.len() + right_jpeg.len());
    mpo.extend_from_slice(&first);
    mpo.extend_from_slice(&right_jpeg);
    Ok((mpo, jpg))
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("Cannot create photo: {e}"))?;
    if let Err(e) = file.write_all(bytes).and_then(|_| file.flush()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(format!("Cannot write photo: {e}"));
    }
    Ok(())
}

/// Save borderless native 400x240 stereo MPO/JPG to the Camera album,
/// plus individual native left/right JPEGs for FTPD. No padded backups.
/// The caller passes clean scene buffers, so notifications are not captured.
pub fn save_stereo_photo(left: Vec<u8>, right: Vec<u8>) -> Result<String, String> {
    let native_l = encode_native_jpeg(&left)?;
    let native_r = encode_native_jpeg(&right)?;
    let (mpo, jpg) = build_photo(&native_l, &native_r)?;
    let folder = Path::new(CAMERA_FOLDER);
    let native_folder = Path::new(NATIVE_FOLDER);
    fs::create_dir_all(folder).map_err(|e| format!("Cannot create DCIM folder: {e}"))?;
    fs::create_dir_all(native_folder)
        .map_err(|e| format!("Cannot create native photo folder: {e}"))?;

    for i in 1..=9999 {
        let stem = format!("HNI_{i:04}");
        let mpo_path = folder.join(format!("{stem}.MPO"));
        let jpg_path = folder.join(format!("{stem}.JPG"));
        let native_l_path = native_folder.join(format!("{stem}_L.JPG"));
        let native_r_path = native_folder.join(format!("{stem}_R.JPG"));
        let paths = [&native_l_path, &native_r_path, &mpo_path, &jpg_path];
        if paths.iter().any(|p| p.exists()) {
            continue;
        }

        // Keep the Camera pair last, so the album never sees a partially
        // created photo unless the final write itself is interrupted.
        let outputs: [(&Path, &[u8]); 4] = [
            (&native_l_path, &native_l),
            (&native_r_path, &native_r),
            (&mpo_path, &mpo),
            (&jpg_path, &jpg),
        ];
        for (index, (path, bytes)) in outputs.iter().enumerate() {
            if let Err(e) = write_bytes(path, bytes) {
                for (made_path, _) in outputs[..index].iter() {
                    let _ = fs::remove_file(made_path);
                }
                return Err(e);
            }
        }
        return Ok(stem);
    }
    Err("3DS Camera photo numbers are full".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn mpo_offsets_point_to_the_actual_second_jpeg() {
        let left = [0xFF, 0xD8, 1, 2, 3, 0xFF, 0xD9];
        let right = [0xFF, 0xD8, 4, 5, 0xFF, 0xD9];
        let (mpo, jpg) = build_photo(&left, &right).unwrap();
        let tiff = 2 + 56 + 8;
        let entries = tiff + 50;
        let primary_len = u32_at(&mpo, entries + 4) as usize;
        let secondary_len = u32_at(&mpo, entries + 16 + 4) as usize;
        let secondary_offset = u32_at(&mpo, entries + 16 + 8) as usize;
        assert_eq!(primary_len, left.len() + 56 + 90);
        assert_eq!(secondary_len, right.len());
        assert_eq!(tiff + secondary_offset, primary_len);
        assert_eq!(&mpo[primary_len..], right);
        assert_eq!(&jpg[2 + 56..], &left[2..]);
        assert_eq!(&mpo[2 + 56 + 90..primary_len], &left[2..]);
    }

    #[test]
    fn rejects_invalid_frames_and_jpegs() {
        assert!(encode_native_jpeg(&[0; 3]).is_err());
        assert!(build_photo(&[0], &[0xFF, 0xD8]).is_err());
        assert!(build_photo(&[0xFF, 0xD8], &[0]).is_err());
    }

    #[test]
    fn calendar_handles_leap_days_and_epoch() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn exclusive_photo_creation_keeps_existing_files() {
        let dir = std::env::temp_dir().join(format!(
            "tg18-photo-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("photo.JPG");
        write_bytes(&path, b"original").unwrap();
        assert!(write_bytes(&path, b"replacement").is_err());
        assert_eq!(fs::read(&path).unwrap(), b"original");
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod encoding_tests {
    use super::*;

    #[test]
    fn camera_files_reuse_the_native_eye_encodings() {
        let mut frame = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
        for x in 0..SCREEN_WIDTH {
            for y in 0..SCREEN_HEIGHT {
                let color = match (x < SCREEN_WIDTH / 2, y < SCREEN_HEIGHT / 2) {
                    (true, true) => [255, 0, 0],
                    (false, true) => [0, 255, 0],
                    (true, false) => [0, 0, 255],
                    (false, false) => [255, 255, 0],
                };
                let dst = (x * SCREEN_HEIGHT + SCREEN_HEIGHT - 1 - y) * 3;
                frame[dst..dst + 3].copy_from_slice(&[color[2], color[1], color[0]]);
            }
        }
        let native = encode_native_jpeg(&frame).unwrap();
        let (mpo, jpg) = build_photo(&native, &native).unwrap();
        let primary_len = native.len() + 56 + 90;
        assert_eq!(&mpo[2 + 56 + 90..primary_len], &native[2..]);
        assert_eq!(&mpo[primary_len..], &native);
        assert_eq!(&jpg[2 + 56..], &native[2..]);
        // Optional files let the host QA independently decode with Pillow.
        if let Ok(dir) = std::env::var("TG18_PHOTO_FIXTURE_DIR") {
            let dir = Path::new(&dir);
            fs::write(dir.join("native.JPG"), native).unwrap();
            fs::write(dir.join("camera.JPG"), jpg).unwrap();
            fs::write(dir.join("camera.MPO"), mpo).unwrap();
        }
    }
}
