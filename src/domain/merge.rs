//! Planning a union of two event sets.
//!
//! Events are immutable, so the same id should always carry the same bytes.
//! When it does not (a converter bug, a hand edit, two versions importing
//! the same legacy data differently), both sides pick the lexicographically
//! smaller bytes. That rule is symmetric, so every replica converges without
//! a human resolving anything, which agents cannot do.

use std::collections::HashMap;

use crate::domain::event::envelope::Event;
use crate::domain::id::EventId;

/// What a merge must write locally.
#[derive(Debug, Default)]
pub struct MergePlan {
    /// Events the local side lacks.
    pub additions: Vec<Event>,
    /// Events whose incoming bytes win over the local bytes.
    pub replacements: Vec<Event>,
    /// Ids whose bytes differed, whichever side won.
    pub conflicts: Vec<EventId>,
}

/// Plans merging `incoming` into a side that holds `local` (id to raw bytes).
///
/// Incoming copies of one id are reduced to their smallest bytes first, so
/// the outcome does not depend on the order sources were read in.
pub fn plan(local: &HashMap<EventId, &str>, incoming: Vec<Event>) -> MergePlan {
    let mut best: HashMap<EventId, Event> = HashMap::with_capacity(incoming.len());
    for event in incoming {
        match best.get(&event.id()) {
            Some(existing) if existing.raw <= event.raw => {}
            Some(_) | None => {
                best.insert(event.id(), event);
            }
        }
    }
    let mut candidates: Vec<Event> = best.into_values().collect();
    candidates.sort_by_key(|event| event.order_key());
    let mut plan = MergePlan::default();
    for event in candidates {
        match local.get(&event.id()) {
            None => plan.additions.push(event),
            Some(existing) if *existing == event.raw => {}
            Some(existing) => {
                plan.conflicts.push(event.id());
                if event.raw.as_str() < *existing {
                    plan.replacements.push(event);
                }
            }
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::plan;
    use crate::domain::event::envelope::Event;
    use crate::error::SillokError;

    fn event(actor: &str) -> Result<Event, SillokError> {
        let raw = format!(
            r#"{{"event_id":"01a0f01e-f6f0-7091-a38c-09f1b8f3c285","event_at":"2026-09-30T00:00:00.000Z","recorded_at":"2026-09-30T00:00:00.000Z","actor":"{actor}","kind":{{"type":"record_restored","record_id":"01a0f01e-f6f0-7091-a38c-09f1b8f3c286"}}}}"#
        );
        Event::parse(raw)
    }

    #[test]
    fn conflicts_converge_on_smaller_bytes() -> Result<(), SillokError> {
        let (a, b) = match (event("a"), event("b")) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(error), _) | (_, Err(error)) => return Err(error),
        };
        let local_a = HashMap::from([(a.id(), a.raw.as_str())]);
        let local_b = HashMap::from([(b.id(), b.raw.as_str())]);
        let into_a = plan(&local_a, vec![b.clone()]);
        let into_b = plan(&local_b, vec![a.clone()]);
        assert!(into_a.replacements.is_empty());
        assert_eq!(into_b.replacements.len(), 1);
        assert_eq!(into_a.conflicts.len(), 1);
        assert_eq!(into_b.conflicts.len(), 1);
        Ok(())
    }

    #[test]
    fn several_incoming_copies_reduce_to_the_smallest() -> Result<(), SillokError> {
        let (a, b, c) = match (event("a"), event("b"), event("c")) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err(error), ..) | (_, Err(error), _) | (.., Err(error)) => return Err(error),
        };
        let local = HashMap::from([(c.id(), c.raw.as_str())]);
        for order in [vec![a.clone(), b.clone()], vec![b.clone(), a.clone()]] {
            let result = plan(&local, order);
            assert_eq!(result.replacements.len(), 1);
            assert_eq!(result.replacements[0].raw, a.raw);
        }
        Ok(())
    }

    #[test]
    fn identical_events_are_ignored() -> Result<(), SillokError> {
        let a = match event("a") {
            Ok(value) => value,
            Err(error) => return Err(error),
        };
        let local = HashMap::from([(a.id(), a.raw.as_str())]);
        let result = plan(&local, vec![a.clone()]);
        assert!(
            result.additions.is_empty()
                && result.replacements.is_empty()
                && result.conflicts.is_empty()
        );
        Ok(())
    }
}
