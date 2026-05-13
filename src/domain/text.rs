use nutype::nutype;

/// Validated task or objective text.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 4096),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, Serialize, Deserialize)
)]
pub struct EntryText(String);

/// Validated purpose text.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 2048),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, Serialize, Deserialize)
)]
pub struct PurposeText(String);

/// Validated tag value.
#[nutype(
    sanitize(trim, lowercase),
    validate(not_empty, len_char_max = 96),
    derive(
        Debug,
        Clone,
        PartialEq,
        Eq,
        PartialOrd,
        Ord,
        Hash,
        AsRef,
        Display,
        Serialize,
        Deserialize
    )
)]
pub struct TagText(String);

/// Validated retraction reason.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 2048),
    derive(Debug, Clone, PartialEq, Eq, AsRef, Display, Serialize, Deserialize)
)]
pub struct ReasonText(String);
