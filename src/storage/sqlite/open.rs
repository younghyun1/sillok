//! Opening, initializing, and version-gating the store.
//!
//! WAL mode lets any number of readers run beside one writer across
//! processes, and `busy_timeout` makes a second writer wait instead of
//! failing, which is what concurrent agents need.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};

use crate::domain::id::ArchiveId;
use crate::domain::time::Timestamp;
use crate::error::SillokError;
use crate::storage::path::ensure_parent;
use crate::storage::sqlite::meta::{
    Layout, StoreInfo, read_info, read_versions, write_info, write_revision,
};
use crate::storage::sqlite::migrate;
use crate::storage::sqlite::schema::{self, SCHEMA_REV, STORE_VERSION};

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
        }
        let mut conn = match connect(path, mode == OpenMode::Create) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let mut layout = match Layout::read(&conn) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        if layout == Layout::Legacy {
            // Migration swaps the database file, so this handle closes first.
            drop(conn);
            match migrate::migrate_v2(path) {
                Ok(Some(report)) => warnings.push(report.summary()),
                Ok(None) => {}
                Err(error) => return Err(error),
            }
            conn = match connect(path, false) {
                Ok(value) => value,
                Err(error) => return Err(error),
            };
            layout = match Layout::read(&conn) {
                Ok(value) => value,
                Err(error) => return Err(error),
            };
        }
        let mut store = Self {
            conn,
            path: path.to_path_buf(),
            warnings,
        };
        let ready = match layout {
            Layout::Current => store.check_version(),
            Layout::Empty => store.create_schema(),
            Layout::Legacy => Err(SillokError::datashape(
                "unsupported_datashape",
                "store is still in the 0.x layout after migration",
            )),
        };
        match ready {
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

    /// Checks the store version and applies additive schema revisions.
    fn check_version(&mut self) -> Result<(), SillokError> {
        let (version, revision) = match read_versions(&self.conn) {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        match version {
            Some(value) if value == STORE_VERSION => {}
            Some(value) => {
                return Err(SillokError::datashape(
                    "unsupported_datashape",
                    format!("store version `{value}` needs a newer sillok"),
                ));
            }
            // A `store_meta` table without a version: an interrupted first
            // write. Creating the schema is idempotent and fills it in.
            None => return self.create_schema(),
        }
        if revision >= SCHEMA_REV {
            return Ok(());
        }
        let tx = match self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
        {
            Ok(value) => value,
            Err(error) => return Err(error.into()),
        };
        if let Err(error) = schema::add(&tx) {
            return Err(error);
        }
        if let Err(error) = write_revision(&tx) {
            return Err(error);
        }
        match tx.commit() {
            Ok(()) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    /// Creates the schema and identity on an empty database.
    fn create_schema(&mut self) -> Result<(), SillokError> {
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
        if let Err(error) = write_revision(&tx) {
            return Err(error);
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
