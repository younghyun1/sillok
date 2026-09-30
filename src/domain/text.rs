//! Validated free-text fields.
//!
//! The character bounds here are mirrored by `CHECK` constraints in the
//! SQLite schema; `storage::sqlite::schema` tests keep the two in agreement.

use std::collections::BTreeSet;

use nutype::nutype;

use crate::error::SillokError;

/// Maximum characters in record text.
pub const ENTRY_MAX_CHARS: usize = 4096;
/// Maximum characters in purpose, note, and reason text.
pub const DETAIL_MAX_CHARS: usize = 2048;
/// Maximum characters in one tag.
pub const TAG_MAX_CHARS: usize = 96;
/// Maximum tags on one record; bounds per-record index rows.
pub const TAGS_MAX: usize = 32;

/// Task or objective text.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 4096),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display)
)]
pub struct EntryText(String);

/// Purpose, completion note, or retraction reason.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 2048),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display)
)]
pub struct DetailText(String);

/// One lowercase tag without separators.
#[nutype(
    sanitize(trim, lowercase),
    validate(not_empty, len_char_max = 96, predicate = |tag: &str| !tag.contains(',') && !tag.chars().any(char::is_whitespace)),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, AsRef, Display)
)]
pub struct TagText(String);

/// Validates record text.
pub fn entry(raw: String) -> Result<String, SillokError> {
    match EntryText::try_new(raw) {
        Ok(value) => Ok(value.into_inner()),
        Err(error) => Err(SillokError::invalid(
            "invalid_text",
            format!("text must be 1-{ENTRY_MAX_CHARS} characters: {error:?}"),
        )),
    }
}

/// Validates purpose, note, or reason text; `field` names it in the error.
pub fn detail(raw: String, field: &'static str) -> Result<String, SillokError> {
    match DetailText::try_new(raw) {
        Ok(value) => Ok(value.into_inner()),
        Err(error) => Err(SillokError::invalid(
            "invalid_text",
            format!("{field} must be 1-{DETAIL_MAX_CHARS} characters: {error:?}"),
        )),
    }
}

/// Validates optional detail text.
pub fn optional_detail(
    raw: Option<String>,
    field: &'static str,
) -> Result<Option<String>, SillokError> {
    match raw {
        Some(value) => match detail(value, field) {
            Ok(clean) => Ok(Some(clean)),
            Err(error) => Err(error),
        },
        None => Ok(None),
    }
}

/// Validates, lowercases, deduplicates, and sorts tags.
pub fn tags(raw: Vec<String>) -> Result<Vec<String>, SillokError> {
    let mut clean = BTreeSet::new();
    for value in raw {
        match TagText::try_new(value) {
            Ok(tag) => {
                clean.insert(tag.into_inner());
            }
            Err(error) => {
                return Err(SillokError::invalid(
                    "invalid_tag",
                    format!(
                        "tags must be 1-{TAG_MAX_CHARS} characters without spaces or commas: {error:?}"
                    ),
                ));
            }
        }
    }
    if clean.len() > TAGS_MAX {
        return Err(SillokError::invalid(
            "invalid_tag",
            format!("at most {TAGS_MAX} tags per record"),
        ));
    }
    Ok(clean.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::{ENTRY_MAX_CHARS, entry, tags};

    #[test]
    fn entry_trims_and_counts_characters() {
        let korean = "실록".repeat(ENTRY_MAX_CHARS / 2);
        assert!(entry(format!("  {korean}  ")).is_ok());
        assert!(entry(format!("{korean}가")).is_err());
        assert!(entry("   ".to_string()).is_err());
    }

    #[test]
    fn tags_normalize() {
        let cleaned = tags(vec!["Rust".into(), "rust".into(), " sync ".into()]);
        assert!(matches!(cleaned, Ok(values) if values == vec!["rust", "sync"]));
    }

    #[test]
    fn tags_reject_separators() {
        assert!(tags(vec!["a b".into()]).is_err());
        assert!(tags(vec!["a,b".into()]).is_err());
    }
}
