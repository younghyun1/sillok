use std::collections::BTreeSet;

use crate::context::capture;
use crate::domain::event::WorkContext;
use crate::domain::text::{EntryText, PurposeText, ReasonText, TagText};
use crate::domain::time::{Timestamp, ZoneChoice};
use crate::error::SillokError;
use crate::storage::handle::StoreHandle;
use crate::storage::path::default_store_path;

/// Runtime values shared by command handlers.
#[derive(Debug, Clone)]
pub struct OperationContext {
    pub store: StoreHandle,
    pub recorded_at: Timestamp,
    pub event_at: Timestamp,
    pub actor: String,
    pub work_context: WorkContext,
    pub zone: ZoneChoice,
    pub warnings: Vec<String>,
}

impl OperationContext {
    /// Builds runtime context from CLI-global fields.
    pub fn new(
        store_path: Option<std::path::PathBuf>,
        at: Option<String>,
        tz: Option<String>,
    ) -> Result<Self, SillokError> {
        let zone = ZoneChoice::parse(tz.as_deref())?;
        let recorded_at = Timestamp::now();
        let event_at = match at {
            Some(value) => zone.parse_timestamp(&value)?,
            None => recorded_at,
        };
        let actor = match std::env::var("SILLOK_ACTOR") {
            Ok(value) if !value.trim().is_empty() => value,
            Ok(_) | Err(_) => "agent".to_string(),
        };
        let (work_context, warnings) = capture::capture();
        let path = match store_path {
            Some(value) => value,
            None => default_store_path()?,
        };
        Ok(Self {
            store: StoreHandle::new(path),
            recorded_at,
            event_at,
            actor,
            work_context,
            zone,
            warnings,
        })
    }

    /// Clones the actor for event construction.
    pub fn actor(&self) -> String {
        self.actor.clone()
    }

    /// Clones the work context for event construction.
    pub fn context(&self) -> WorkContext {
        self.work_context.clone()
    }
}

/// Validates task/objective text and returns the sanitized string.
pub fn clean_entry(raw: String) -> Result<String, SillokError> {
    match EntryText::try_new(raw) {
        Ok(value) => Ok(value.into_inner()),
        Err(error) => Err(SillokError::new(
            "invalid_text",
            format!("invalid entry text: {error:?}"),
        )),
    }
}

/// Validates optional purpose text.
pub fn clean_purpose(raw: Option<String>) -> Result<Option<String>, SillokError> {
    match raw {
        Some(value) => match PurposeText::try_new(value) {
            Ok(value) => Ok(Some(value.into_inner())),
            Err(error) => Err(SillokError::new(
                "invalid_purpose",
                format!("invalid purpose text: {error:?}"),
            )),
        },
        None => Ok(None),
    }
}

/// Validates and deduplicates tags.
pub fn clean_tags(raw: Vec<String>) -> Result<Vec<String>, SillokError> {
    let mut tags = BTreeSet::new();
    for value in raw {
        match TagText::try_new(value) {
            Ok(tag) => {
                tags.insert(tag.into_inner());
            }
            Err(error) => {
                return Err(SillokError::new(
                    "invalid_tag",
                    format!("invalid tag: {error:?}"),
                ));
            }
        }
    }
    Ok(tags.into_iter().collect())
}

/// Validates retraction reason text.
pub fn clean_reason(raw: String) -> Result<String, SillokError> {
    match ReasonText::try_new(raw) {
        Ok(value) => Ok(value.into_inner()),
        Err(error) => Err(SillokError::new(
            "invalid_reason",
            format!("invalid reason: {error:?}"),
        )),
    }
}
