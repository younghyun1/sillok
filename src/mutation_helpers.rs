use crate::domain::event::{ChronicleEvent, EventKind, RecordStatus};
use crate::domain::id::ChronicleId;
use crate::domain::time::DayKey;
use crate::domain::view::{ChronicleView, DerivedRecord};
use crate::error::SillokError;
use crate::operation::OperationContext;

type ParentTarget = (ChronicleId, ChronicleId, Option<(ChronicleId, DayKey)>);

/// Parses an optional UUID argument.
pub fn parse_optional_id(raw: Option<String>) -> Result<Option<ChronicleId>, SillokError> {
    match raw {
        Some(value) => match ChronicleId::parse(&value) {
            Ok(id) => Ok(Some(id)),
            Err(error) => Err(error),
        },
        None => Ok(None),
    }
}

/// Resolves the target day and parent for a new task.
pub fn parent_target(
    ctx: &OperationContext,
    view: &ChronicleView<'_>,
    parent: Option<ChronicleId>,
) -> Result<ParentTarget, SillokError> {
    match parent {
        Some(parent_id) => match require_active_record(view, parent_id) {
            Ok(parent_record) => Ok((parent_record.day_id, parent_id, None)),
            Err(error) => Err(error),
        },
        None => match ctx.zone.day_key(ctx.event_at) {
            Ok(day_key) => {
                let (day_id, day_to_open) = day_for_key(view, day_key);
                Ok((day_id, day_id, day_to_open))
            }
            Err(error) => Err(error),
        },
    }
}

/// Finds or allocates a day id for a day key.
pub fn day_for_key(
    view: &ChronicleView<'_>,
    day_key: DayKey,
) -> (ChronicleId, Option<(ChronicleId, DayKey)>) {
    match view.day_id(&day_key) {
        Some(day_id) => (day_id, None),
        None => {
            let day_id = ChronicleId::new_v7();
            (day_id, Some((day_id, day_key)))
        }
    }
}

/// Builds a `DayOpened` event.
pub fn day_opened_event(
    ctx: &OperationContext,
    day_id: ChronicleId,
    day_key: DayKey,
) -> ChronicleEvent {
    ChronicleEvent::new(
        ctx.event_at,
        ctx.recorded_at,
        ctx.actor(),
        ctx.context(),
        EventKind::DayOpened { day_id, day_key },
    )
}

/// Requires an existing non-retracted record.
pub fn require_active_record(
    view: &ChronicleView<'_>,
    record_id: ChronicleId,
) -> Result<DerivedRecord, SillokError> {
    match require_output_record(view, record_id) {
        Ok(record) => {
            if record.status == RecordStatus::Retracted {
                Err(SillokError::new(
                    "record_retracted",
                    format!("record `{record_id}` has been retracted"),
                ))
            } else {
                Ok(record)
            }
        }
        Err(error) => Err(error),
    }
}

/// Requires an existing record and clones it for output.
pub fn require_output_record(
    view: &ChronicleView<'_>,
    record_id: ChronicleId,
) -> Result<DerivedRecord, SillokError> {
    match view.record(&record_id) {
        Some(record) => Ok(record.clone()),
        None => Err(SillokError::new(
            "record_not_found",
            format!("record `{record_id}` does not exist"),
        )),
    }
}
