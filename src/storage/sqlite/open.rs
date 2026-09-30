//! Opening, initializing, and version-gating the store.
//!
//! WAL mode lets any number of readers run beside one writer across
//! processes, and `busy_timeout` makes a second writer wait instead of
//! failing, which is what concurrent agents need.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

use crate::domain::id::ArchiveId;
use crate::domain::time::Timestamp;
use crate::error::SillokError;
use crate::legacy::v2_store::is_v2_store;
use crate::storage::path::ensure_parent;
use crate::storage::sqlite::migrate;
use crate::storage::sqlite::schema::{self, STORE_VERSION};

/// How long a writer waits for another writer before `store_busy`.
pub const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether opening may create a missing store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenMode {
    /// Missing stores stay missing; reads report empty results.
    Existing,
    /// Missing stores are created.
    Create,
}

/// Identity stored in `store_meta`.
#[derive(Debug, Clone, Copy)]
pub struct StoreInfo {
    pub archive_id: ArchiveId,
    pub created_at: Timestamp,
}

/// One open store connection for the duration of a command.
#[derive(Debug)]
pub struct Store {
    pub(crate) conn: Connection,
    path: PathBuf,
    /// Notices for the caller, such as a migration that just ran.
    pub warnings: Vec<String>,
}

impl Store {
    /// Opens the store, migrating a 0.9/0.10 store in place first.
    pub fn open(path: &Path, mode: OpenMode) -> Result<Option<Self>, SillokError> {
        let mut warnings = Vec::new();
        if !path.exists() {
            if mode == OpenMode::Existing {
                return Ok(None);
            }
            if let Err(error) = ensure_parent(path) {
                return Err(error);
            }
        } else {
            match needs_migration(path) {
                Ok(true) => match migrate::migrate_v2(path) {
                    Ok(Some(report)) => warnings.push(report.summary()),
                    Ok(None) => {}
                    Err(error) => return Err(error),
                },
                Ok(false) => {}
                Err(error) => return Err(error),
            }
        }
        let conn = match connect(path, mode == OpenMode::Create) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let mut store = Self {
            conn,
            path: path.to_path_buf(),
            warnings,
        };
        match store.ensure_schema() {
            Ok(()) => Ok(Some(store)),
            Err(error) => Err(error),
        }
    }

    /// Store path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads the archive identity.
    pub fn info(&self) -> Result<StoreInfo, SillokError> {
        read_info(&self.conn)
    }

    /// Replaces the archive identity (sync adopts the older lineage).
    pub fn set_info(&self, info: StoreInfo) -> Result<(), SillokError> {
        write_info(&self.conn, info)
    }

    /// Creates the schema on an empty file and checks the version otherwise.
    fn ensure_schema(&mut self) -> Result<(), SillokError> {
        match meta(&self.conn, "store_version") {
            Ok(Some(version)) if version == STORE_VERSION => return Ok(()),
            Ok(Some(version)) => {
                return Err(SillokError::datashape(
                    "unsupported_datashape",
                    format!("store version `{version}` needs a newer sillok"),
                ));
            }
            Ok(None) => {}
            Err(error) => return Err(error),
        }
        // WAL is persistent in the file, so this runs once per store.
        if let Err(error) = self.conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        }) {
            return Err(error.into());
        }
        let tx = match self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
        {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        if let Err(error) = schema::create(&tx) {
            return Err(error);
        }
        let info = StoreInfo {
            archive_id: ArchiveId::new_v7(),
            created_at: Timestamp::now(),
        };
        // OR IGNORE: a concurrent first writer may have initialized already.
        for (key, value) in [
            ("store_version", STORE_VERSION.to_string()),
            ("archive_id", info.archive_id.to_string()),
            ("created_at_ms", info.created_at.as_millis().to_string()),
        ] {
            if let Err(error) = tx.execute(
                "INSERT OR IGNORE INTO store_meta (store_meta_key, store_meta_value) VALUES (?1, ?2)",
                params![key, value],
            ) {
                return Err(error.into());
            }
        }
        match tx.commit() {
            Ok(()) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

/// Opens a configured connection; `create` allows creating a missing file.
pub fn connect(path: &Path, create: bool) -> Result<Connection, SillokError> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let conn = match Connection::open_with_flags(path, flags) {
        Ok(value) => value,
        Err(error) => return Err(error.into()),
    };
    if let Err(error) = conn.busy_timeout(BUSY_TIMEOUT) {
        return Err(error.into());
    }
    // FULL: a chronicle entry that returned an id must survive power loss.
    if let Err(error) = conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;") {
        return Err(error.into());
    }
    conn.set_prepared_statement_cache_capacity(64);
    Ok(conn)
}

fn needs_migration(path: &Path) -> Result<bool, SillokError> {
    let conn = match connect(path, false) {
        Ok(value) => value,
        Err(error) => return Err(error),
    };
    is_v2_store(&conn)
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
