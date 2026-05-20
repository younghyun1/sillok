use crate::domain::archive::Archive;
use crate::error::SillokError;

pub const LEGACY_ZSTD_LEVEL: i32 = 3;
pub const SYNC_ZSTD_LEVEL: i32 = 22;

/// Encodes an archive as bitcode bytes compressed with zstd at the supplied level.
pub fn encode_archive(archive: &Archive, zstd_level: i32) -> Result<Vec<u8>, SillokError> {
    let encoded = bitcode::encode(archive);
    let compressed = zstd::stream::encode_all(&encoded[..], zstd_level)?;
    drop(encoded);
    Ok(compressed)
}

/// Decodes a zstd-compressed bitcode archive.
pub fn decode_archive(bytes: &[u8]) -> Result<Archive, SillokError> {
    let encoded = zstd::stream::decode_all(bytes)?;
    let archive = bitcode::decode::<Archive>(&encoded)?;
    drop(encoded);
    Ok(archive)
}

/// Encodes a sync artifact with maximum zstd compression.
pub fn encode_sync_archive(archive: &Archive) -> Result<Vec<u8>, SillokError> {
    encode_archive(archive, SYNC_ZSTD_LEVEL)
}

#[cfg(test)]
mod tests {
    use crate::archive_codec::{decode_archive, encode_sync_archive};
    use crate::domain::archive::Archive;
    use crate::domain::event::WorkContext;
    use crate::domain::time::Timestamp;

    fn context() -> WorkContext {
        WorkContext {
            cwd: Some("/tmp/sillok-codec-test".to_string()),
            git_root: None,
            git_branch: None,
            git_head: None,
            git_remote: None,
        }
    }

    #[test]
    fn sync_codec_roundtrips_at_level_22() -> Result<(), Box<dyn std::error::Error>> {
        let archive = Archive::new(Timestamp::from_millis(42), "test".to_string(), context());
        let encoded = encode_sync_archive(&archive)?;
        let decoded = decode_archive(&encoded)?;
        assert_eq!(decoded, archive);
        Ok(())
    }
}
