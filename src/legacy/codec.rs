//! Decoder for 0.x `.slk.zst` files (bitcode inside zstd).

use std::io::Read;

use crate::error::SillokError;
use crate::legacy::types::LegacyArchive;

/// zstd frame magic, little-endian.
pub const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

/// Largest decompressed archive accepted. A remote artifact is not fully
/// trusted input, so decompression is bounded to stop a zstd bomb.
pub const MAX_DECODED_BYTES: u64 = 512 * 1024 * 1024;

/// Decodes a v1 store file or v2 sync artifact.
pub fn decode_archive(bytes: &[u8]) -> Result<LegacyArchive, SillokError> {
    let decoder = match zstd::stream::read::Decoder::new(bytes) {
        Ok(value) => value,
        Err(error) => return Err(SillokError::LegacyDecode(error.to_string())),
    };
    let mut decoded = Vec::new();
    match decoder
        .take(MAX_DECODED_BYTES + 1)
        .read_to_end(&mut decoded)
    {
        Ok(_) => {}
        Err(error) => return Err(SillokError::LegacyDecode(error.to_string())),
    }
    if decoded.len() as u64 > MAX_DECODED_BYTES {
        return Err(SillokError::LegacyDecode(format!(
            "archive exceeds {MAX_DECODED_BYTES} decoded bytes"
        )));
    }
    let archive = match bitcode::decode::<LegacyArchive>(&decoded) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    drop(decoded);
    if archive.schema_version != 1 {
        return Err(SillokError::datashape(
            "unsupported_datashape",
            format!(
                "legacy archive schema {} is not supported",
                archive.schema_version
            ),
        ));
    }
    Ok(archive)
}

/// Whether bytes start with a zstd frame.
pub fn is_zstd(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == ZSTD_MAGIC
}
