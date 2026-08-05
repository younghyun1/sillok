use crate::domain::event::RecordStatus;
use crate::domain::time::Timestamp;
use crate::domain::view::{ChronicleView, DerivedRecord};

/// Filter bounds shared by the record-query paths.
#[derive(Debug, Clone, Copy)]
struct RecordQuery<'a> {
    from: Timestamp,
    to: Timestamp,
    context: Option<&'a str>,
    tag: Option<&'a str>,
    status: Option<RecordStatus>,
}

impl ChronicleView<'_> {
    /// Returns visible records within an inclusive creation-time range.
    pub fn query(
        &self,
        from: Timestamp,
        to: Timestamp,
        context: Option<&str>,
        tag: Option<&str>,
        status: Option<RecordStatus>,
    ) -> Vec<DerivedRecord> {
        let mut records = Vec::new();
        let filter = RecordQuery {
            from,
            to,
            context,
            tag,
            status,
        };
        match (filter.tag, filter.status) {
            (Some(required_tag), Some(required_status)) => {
                let Some(tag_ids) = self.by_tag.get(required_tag) else {
                    return records;
                };
                let Some(status_ids) = self.by_status.get(&required_status) else {
                    return records;
                };
                if tag_ids.len() <= status_ids.len() {
                    self.push_matching_records(tag_ids, &mut records, filter);
                } else {
                    self.push_matching_records(status_ids, &mut records, filter);
                }
            }
            (Some(required_tag), None) => {
                let Some(ids) = self.by_tag.get(required_tag) else {
                    return records;
                };
                self.push_matching_records(ids, &mut records, filter);
            }
            (None, Some(required_status)) => {
                let Some(ids) = self.by_status.get(&required_status) else {
                    return records;
                };
                self.push_matching_records(ids, &mut records, filter);
            }
            (None, None) => {
                for (_timestamp, bucket) in self.timeline.range(filter.from..=filter.to) {
                    self.push_matching_records(bucket, &mut records, filter);
                }
            }
        }
        sort_records(&mut records);
        records
    }

    fn push_matching_records(
        &self,
        ids: &[crate::domain::id::ChronicleId],
        records: &mut Vec<DerivedRecord>,
        filter: RecordQuery<'_>,
    ) {
        for id in ids {
            let Some(record) = self.records.get(id) else {
                continue;
            };
            if record_matches_query(record, filter) {
                records.push(record.clone());
            }
        }
    }
}

fn sort_records(records: &mut [DerivedRecord]) {
    records.sort_by_key(|record| (record.created_at, record.record_id));
}

fn record_matches_query(record: &DerivedRecord, filter: RecordQuery<'_>) -> bool {
    if record.created_at < filter.from || record.created_at > filter.to {
        return false;
    }
    if record.status == RecordStatus::Retracted {
        return false;
    }
    if let Some(required_status) = filter.status
        && record.status != required_status
    {
        return false;
    }
    if let Some(required_tag) = filter.tag
        && !record.tags.iter().any(|value| value == required_tag)
    {
        return false;
    }
    if let Some(required_context) = filter.context
        && !record.context.key_contains(required_context)
    {
        return false;
    }
    true
}
