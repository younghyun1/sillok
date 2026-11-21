//! `store_meta`: the store's layout, version, and identity.

use rusqlite::{Connection, OptionalExtension, params};

use crate::domain::id::ArchiveId;
use crate::domain::time::Timestamp;
use crate::error::SillokError;
use crate::storage::sqlite::schema::SCHEMA_REV;

/// Identity stored in `store_meta`.
#[derive(Debug, Clone, Copy)]
pub struct StoreInfo {
    pub archive_id: ArchiveId,
    pub created_at: Timestamp,
}

/// What kind of database a file holds, read with one catalog query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// No Sillok tables yet.
    Empty,
    /// A 0.9/0.10 store awaiting migration.
    Legacy,
    /// A 1.x store.
    Current,
}

impl Layout {
    /// Classifies the database behind `conn`.
    pub fn read(conn: &Connection) -> Result<Self, SillokError> {
        let mut statement = match conn.prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name IN ('sillok_meta', 'store_meta')",
        ) {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let rows = match statement.query_map([], |row| row.get::<_, String>(0)) {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        let mut layout = Self::Empty;
        for row in rows {
            match row {
                // The 0.x marker wins: such a store must migrate before use.
                Ok(name) if name == "sillok_meta" => return Ok(Self::Legacy),
                Ok(_) => layout = Self::Current,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(layout)
    }
}

/// Reads `store_version` and `schema_rev` in one query. A missing or
/// unreadable revision counts as 0, which re-applies the idempotent additions.
pub fn read_versions(conn: &Connection) -> Result<(Option<String>, u32), SillokError> {
    let mut statement = match conn.prepare(
        "SELECT store_meta_key, store_meta_value FROM store_meta
         WHERE store_meta_key IN ('store_version', 'schema_rev')",
    ) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let rows = match statement.query_map([], |row| {
        Ok((row.get::<_, String>(0), row.get::<_, String>(1)))
    }) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    let mut version = None;
    let mut revision = 0;
    for row in rows {
        match row {
            Ok((Ok(key), Ok(value))) if key == "store_version" => version = Some(value),
            Ok((Ok(_), Ok(value))) => {
                revision = match value.parse::<u32>() {
                    Ok(parsed) => parsed,
                    Err(_) => 0,
                };
            }
            Ok((Err(error), _)) | Ok((_, Err(error))) | Err(error) => return Err(error.into()),
        }
    }
    Ok((version, revision))
}

/// Records that this binary's additive schema is in place. A newer binary's
/// higher revision is kept, so two versions sharing a store do not flap.
pub fn write_revision(conn: &Connection) -> Result<(), SillokError> {
    match conn.execute(
        "INSERT INTO store_meta (store_meta_key, store_meta_value) VALUES ('schema_rev', ?1)
         ON CONFLICT (store_meta_key) DO UPDATE SET store_meta_value = excluded.store_meta_value
         WHERE CAST(store_meta.store_meta_value AS INTEGER) < CAST(excluded.store_meta_value AS INTEGER)",
        params![SCHEMA_REV.to_string()],
    ) {
        Ok(_) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Reads a `store_meta` value; a missing table reads as absent.
pub fn meta(conn: &Connection, key: &str) -> Result<Option<String>, SillokError> {
    let exists: Option<String> = match conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'store_meta'",
            [],
            |row| row.get(0),
        )
        .optional()
    {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    if exists.is_none() {
        return Ok(None);
    }
    match conn
        .query_row(
            "SELECT store_meta_value FROM store_meta WHERE store_meta_key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
    {
        Ok(value) => Ok(value),
        Err(error) => Err(error.into()),
    }
}

/// Reads the archive identity from `store_meta`.
pub fn read_info(conn: &Connection) -> Result<StoreInfo, SillokError> {
    let archive_id = match meta(conn, "archive_id") {
        Ok(Some(raw)) => match ArchiveId::parse(&raw) {
            Ok(value) => value,
            Err(error) => return Err(error),
        },
        Ok(None) => {
            return Err(SillokError::datashape(
                "invalid_datashape",
                "store has no archive id",
            ));
        }
        Err(error) => return Err(error),
    };
    let created_at = match meta(conn, "created_at_ms") {
        Ok(Some(raw)) => match raw.parse::<i64>() {
            Ok(value) => Timestamp::from_millis(value),
            Err(error) => {
                return Err(SillokError::datashape(
                    "invalid_datashape",
                    error.to_string(),
                ));
            }
        },
        Ok(None) => {
            return Err(SillokError::datashape(
                "invalid_datashape",
                "store has no creation time",
            ));
        }
        Err(error) => return Err(error),
    };
    Ok(StoreInfo {
        archive_id,
        created_at,
    })
}

/// Writes the archive identity.
pub fn write_info(conn: &Connection, info: StoreInfo) -> Result<(), SillokError> {
    for (key, value) in [
        ("archive_id", info.archive_id.to_string()),
        ("created_at_ms", info.created_at.as_millis().to_string()),
    ] {
        if let Err(error) = conn.execute(
            "INSERT INTO store_meta (store_meta_key, store_meta_value) VALUES (?1, ?2)
             ON CONFLICT (store_meta_key) DO UPDATE SET store_meta_value = excluded.store_meta_value",
            params![key, value],
        ) {
            return Err(error.into());
        }
    }
    Ok(())
}
