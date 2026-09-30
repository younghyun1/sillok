//! Per-invocation context shared by command handlers.

use std::path::PathBuf;

use chrono::NaiveDate;

use crate::context::capture;
use crate::domain::id::RecordId;
use crate::domain::time::Timestamp;
use crate::domain::zone::Zone;
use crate::error::SillokError;
use crate::storage::path::default_store_path;
use crate::storage::sqlite::open::{OpenMode, Store};
use crate::storage::sqlite::writes::Stamp;

/// Environment variable naming who records events.
pub const ACTOR_ENV: &str = "SILLOK_ACTOR";

/// Resolved global options.
#[derive(Debug)]
pub struct Ctx {
    pub store_path: PathBuf,
    pub zone: Zone,
    /// `--at`, when given.
    pub at: Option<Timestamp>,
    pub now: Timestamp,
    pub full: bool,
    pub warnings: Vec<String>,
}

impl Ctx {
    /// Resolves the store path, timezone, and `--at`.
    pub fn new(
        store: Option<PathBuf>,
        tz: Option<String>,
        at: Option<String>,
        full: bool,
    ) -> Result<Self, SillokError> {
        let mut warnings = Vec::new();
        let store_path = match store {
            Some(path) if !path.as_os_str().is_empty() => path,
            Some(_) | None => match default_store_path() {
                Ok(value) => value,
                Err(error) => return Err(error),
            },
        };
        let tz = tz.filter(|value| !value.trim().is_empty());
        let zone = match Zone::resolve(tz.as_deref()) {
            Ok((zone, warning)) => {
                if let Some(message) = warning {
                    warnings.push(message);
                }
                zone
            }
            Err(error) => return Err(error),
        };
        let at = match at {
            Some(raw) => match zone.parse_instant(&raw) {
                Ok(value) => Some(value),
                Err(error) => return Err(error),
            },
            None => None,
        };
        Ok(Self {
            store_path,
            zone,
            at,
            now: Timestamp::now(),
            full,
            warnings,
        })
    }

    /// Opens an existing store; `None` when it does not exist yet.
    pub fn open(&mut self) -> Result<Option<Store>, SillokError> {
        match Store::open(&self.store_path, OpenMode::Existing) {
            Ok(Some(mut store)) => {
                self.warnings.append(&mut store.warnings);
                Ok(Some(store))
            }
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Opens the store, creating it when missing.
    pub fn open_or_create(&mut self) -> Result<Store, SillokError> {
        match Store::open(&self.store_path, OpenMode::Create) {
            Ok(Some(mut store)) => {
                self.warnings.append(&mut store.warnings);
                Ok(store)
            }
            Ok(None) => Err(SillokError::datashape(
                "store_missing",
                "store could not be created",
            )),
            Err(error) => Err(error),
        }
    }

    /// Who, when, and where for a new event.
    pub fn stamp(&mut self) -> Stamp {
        let (context, mut warnings) = capture::capture();
        self.warnings.append(&mut warnings);
        let actor = match std::env::var(ACTOR_ENV) {
            Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
            Ok(_) | Err(_) => "agent".to_string(),
        };
        Stamp {
            event_at: match self.at {
                Some(value) => value,
                None => self.now,
            },
            recorded_at: self.now,
            backfilled: self.at.is_some(),
            actor,
            context,
        }
    }

    /// Local date named by `raw`, or the date of `--at`/now.
    pub fn date(&self, raw: Option<&str>) -> Result<NaiveDate, SillokError> {
        match raw {
            Some(value) => self.zone.parse_date(value),
            None => self.zone.date_of(match self.at {
                Some(value) => value,
                None => self.now,
            }),
        }
    }

    /// Parses an optional time bound.
    pub fn instant(&self, raw: Option<&str>) -> Result<Option<Timestamp>, SillokError> {
        match raw {
            Some(value) => match self.zone.parse_instant(value) {
                Ok(parsed) => Ok(Some(parsed)),
                Err(error) => Err(error),
            },
            None => Ok(None),
        }
    }
}

/// Parses an optional record id argument.
pub fn optional_id(raw: Option<&str>) -> Result<Option<RecordId>, SillokError> {
    match raw {
        Some(value) => match RecordId::parse(value) {
            Ok(id) => Ok(Some(id)),
            Err(error) => Err(error),
        },
        None => Ok(None),
    }
}
