//! Complete writes before replacing an existing image or configuration file.
use std::os::windows::ffi::OsStrExt;
use std::{
    fs::{File, OpenOptions},
    io::{self, BufWriter, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
use windows_sys::Win32::Storage::FileSystem::*;

pub fn encode_png(
    mut writer: impl Write,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), String> {
    encode_rgba(&mut writer, width, height, rgba).map_err(|e| e.to_string())
}

// Screenshots only need non-interlaced RGBA8. Keep the same Sub filter and fast
// compressor as png::Encoder's defaults without linking its other formats.
fn encode_rgba(writer: &mut impl Write, width: u32, height: u32, rgba: &[u8]) -> io::Result<()> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid RGBA image dimensions");
    if width == 0 || height == 0 || width > i32::MAX as u32 || height > i32::MAX as u32 {
        return Err(invalid());
    }
    let row_len = (width as usize).checked_mul(4).ok_or_else(invalid)?;
    if row_len.checked_mul(height as usize) != Some(rgba.len()) {
        return Err(invalid());
    }
    let filtered_len = row_len.checked_add(1).ok_or_else(invalid)?;
    let raw_size = filtered_len
        .checked_mul(height as usize)
        .ok_or_else(invalid)?;
    let mut compressor = fdeflate::Compressor::new(Vec::new())?;
    let mut filtered = vec![0; filtered_len];
    filtered[0] = 1; // PNG Sub: subtract the corresponding byte of the left pixel.
    for row in rgba.chunks_exact(row_len) {
        filtered[1..5].copy_from_slice(&row[..4]);
        for (out, pair) in filtered[5..].iter_mut().zip(row[4..].iter().zip(row)) {
            *out = pair.0.wrapping_sub(*pair.1);
        }
        compressor.write_data(&filtered)?;
    }
    let mut compressed = compressor.finish()?;
    // Preserve the previous encoder's fallback for incompressible screenshots.
    if compressed.len() > fdeflate::StoredOnlyCompressor::<()>::compressed_size(raw_size) {
        compressed.clear();
        let mut stored = fdeflate::StoredOnlyCompressor::new(io::Cursor::new(compressed))?;
        for row in rgba.chunks_exact(row_len) {
            stored.write_data(&[0])?;
            stored.write_data(row)?;
        }
        compressed = stored.finish()?.into_inner();
    }
    writer.write_all(b"\x89PNG\r\n\x1a\n")?;
    let mut header = [0u8; 13];
    header[..4].copy_from_slice(&width.to_be_bytes());
    header[4..8].copy_from_slice(&height.to_be_bytes());
    header[8] = 8;
    header[9] = 6; // RGBA
    png_chunk(writer, b"IHDR", &header)?;
    for chunk in compressed.chunks(64 * 1024) {
        png_chunk(writer, b"IDAT", chunk)?;
    }
    png_chunk(writer, b"IEND", &[])?;
    writer.flush()
}

fn png_chunk(writer: &mut impl Write, kind: &[u8; 4], data: &[u8]) -> io::Result<()> {
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    writer.write_all(&(data.len() as u32).to_be_bytes())?;
    writer.write_all(kind)?;
    writer.write_all(data)?;
    writer.write_all(&crc.finalize().to_be_bytes())
}

pub fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut File) -> Result<(), String>,
) -> Result<(), String> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let (temp, mut file) = loop {
        let temp = parent.join(format!(
            ".qingjie-{}-{}.tmp",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    };
    let result = write(&mut file).and_then(|()| file.sync_all().map_err(|e| e.to_string()));
    drop(file);
    let result = result.and_then(|()| {
        let from: Vec<_> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            Err(io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    });
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
pub fn png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    atomic_write(path, |file| {
        encode_png(BufWriter::new(file), width, height, rgba)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reference_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(rgba).unwrap();
        writer.finish().unwrap();
        bytes
    }
    fn idat(bytes: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut chunks = &bytes[8..];
        while !chunks.is_empty() {
            let len = u32::from_be_bytes(chunks[..4].try_into().unwrap()) as usize;
            if &chunks[4..8] == b"IDAT" {
                data.extend_from_slice(&chunks[8..8 + len]);
            }
            chunks = &chunks[12 + len..];
        }
        data
    }
    fn fixture(width: u32, height: u32, noise: bool) -> Vec<u8> {
        let mut seed = 0x12345678u32;
        (0..width * height * 4)
            .map(|i| {
                if noise {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as u8
                } else if i % 4 == 3 {
                    255
                } else {
                    ((i / 4 / width / 24 + i / 4 % width / 96) % 8 * 30) as u8
                }
            })
            .collect()
    }
    #[test]
    fn png_pixels_and_compression_match_reference() {
        for (width, height) in [(1, 1), (1, 37), (37, 1), (257, 263)] {
            for noise in [false, true] {
                let pixels = fixture(width, height, noise);
                let mut bytes = Vec::new();
                encode_png(&mut bytes, width, height, &pixels).unwrap();
                // Includes a noisy image larger than one IDAT and arbitrary alpha.
                let mut reader = png::Decoder::new(bytes.as_slice()).read_info().unwrap();
                let mut decoded = vec![0; reader.output_buffer_size()];
                let info = reader.next_frame(&mut decoded).unwrap();
                assert_eq!((info.width, info.height), (width, height));
                assert_eq!(decoded, pixels);
                assert_eq!(idat(&bytes), idat(&reference_png(width, height, &pixels)));
            }
        }
    }
    #[test]
    fn invalid_png_input_is_rejected_before_writing() {
        for (w, h, pixels) in [
            (0, 1, &[][..]),
            (1, 0, &[][..]),
            (u32::MAX, 1, &[][..]),
            (1, 1, &[0; 3][..]),
            (1, 1, &[0; 5][..]),
        ] {
            let mut bytes = Vec::new();
            assert!(encode_png(&mut bytes, w, h, pixels).is_err());
            assert!(bytes.is_empty());
        }
    }
    #[test]
    fn png_flush_failure_is_reported() {
        struct FailedFlush;
        impl Write for FailedFlush {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Err(io::Error::other("flush failed"))
            }
        }
        assert!(encode_png(FailedFlush, 1, 1, &[0; 4]).is_err());
    }
    #[test]
    #[ignore = "manual release benchmark; timings are not correctness assertions"]
    fn compare_png_encoding_cost() {
        for noise in [false, true] {
            let pixels = fixture(1920, 1080, noise);
            for reference in [true, false] {
                let start = std::time::Instant::now();
                let mut size = 0;
                for _ in 0..12 {
                    let bytes = if reference {
                        reference_png(1920, 1080, &pixels)
                    } else {
                        let mut bytes = Vec::new();
                        encode_png(&mut bytes, 1920, 1080, &pixels).unwrap();
                        bytes
                    };
                    size = std::hint::black_box(bytes).len();
                }
                eprintln!(
                    "noise={noise} reference={reference} bytes={size} average={:?}",
                    start.elapsed() / 12
                );
            }
        }
    }
    struct FailedDisk;
    impl Write for FailedDisk {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("disk full"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("flush failed"))
        }
    }
    #[test]
    fn buffered_write_failure_is_reported() {
        assert!(encode_png(BufWriter::new(FailedDisk), 1, 1, &[255; 4]).is_err());
    }
    #[test]
    fn failed_export_preserves_original_and_cleans_temp() {
        let dir = std::env::current_dir().unwrap().join("target/export-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("existing.png");
        std::fs::write(&path, b"original").unwrap();
        assert!(atomic_write(&path, |file| {
            file.write_all(b"partial").unwrap();
            Err("injected failure".into())
        })
        .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        png(&path, 1, 1, &[10, 20, 30, 255]).unwrap();
        let mut reader = png::Decoder::new(File::open(path).unwrap())
            .read_info()
            .unwrap();
        let mut bytes = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut bytes).unwrap();
        assert_eq!(bytes, [10, 20, 30, 255]);
    }
}
