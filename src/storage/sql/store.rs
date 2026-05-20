use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use turso::transaction::{Transaction, TransactionBehavior};
use turso::{Builder, Connection, Row, Value, params, params_from_iter};

use crate::domain::archive::{ARCHIVE_SCHEMA_VERSION, Archive};
use crate::domain::event::{ChronicleEvent, EventKind, RecordKind, RecordStatus, WorkContext};
use crate::domain::id::ChronicleId;
use crate::domain::reducer;
use crate::domain::time::{DayKey, Timestamp};
use crate::domain::view::{ChronicleView, DerivedRecord, RecordTreeNode};
use crate::error::SillokError;
use crate::storage::sql::convert;
use crate::storage::sql::schema::{
    EVENT_DATASHAPE_VERSION, RECORD_DATASHAPE_VERSION, STORE_DATASHAPE_VERSION,
};

/// Summary of a v2 SQL-backed store.
#[derive(Debug, Clone)]
pub struct StoreInfo {
    pub archive_id: ChronicleId,
    pub created_at: Timestamp,
}

/// Validation and sizing details for a v2 SQL-backed store.
#[derive(Debug, Clone)]
pub struct StoreStats {
    pub info: StoreInfo,
    pub event_count: usize,
    pub record_count: usize,
}

/// Turso-backed v2 chronicle store.
#[derive(Debug, Clone)]
pub struct SqlStore {
    path: PathBuf,
}

impl SqlStore {
    /// Creates a store wrapper for a database path.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Returns the database path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Initializes the database if it is absent.
    pub async fn init(
        &self,
        recorded_at: Timestamp,
        actor: String,
        context: WorkContext,
    ) -> Result<(StoreInfo, bool), SillokError> {
        let existed = self.path.exists();
        let mut conn = self.connect().await?;
        let created = ensure_initialized(&mut conn, recorded_at, actor, context).await?;
        let info = read_info(&conn).await?;
        Ok((info, created || !existed))
    }

    /// Reads store stats when the database exists.
    pub async fn stats(&self) -> Result<Option<StoreStats>, SillokError> {
        let Some(conn) = self.connect_existing().await? else {
            return Ok(None);
        };
        let info = read_info(&conn).await?;
        let event_count = count_rows(&conn, "events").await?;
        let record_count = count_rows(&conn, "records").await?;
        Ok(Some(StoreStats {
            info,
            event_count,
            record_count,
        }))
    }

    /// Exports the authoritative append-only event stream from the database.
    pub async fn export_archive(&self) -> Result<Option<Archive>, SillokError> {
        let Some(conn) = self.connect_existing().await? else {
            return Ok(None);
        };
        let archive = archive_from_db(&conn).await?;
        ChronicleView::build(&archive)?;
        Ok(Some(archive))
    }

    /// Reads one record and its event history.
    pub async fn show(
        &self,
        record_id: ChronicleId,
    ) -> Result<Option<(DerivedRecord, Vec<ChronicleEvent>)>, SillokError> {
        let Some(conn) = self.connect_existing().await? else {
            return Ok(None);
        };
        let Some(record) = fetch_record(&conn, record_id).await? else {
            return Ok(None);
        };
        let events = events_for_record(&conn, record_id).await?;
        Ok(Some((record, events)))
    }

    /// Reads one day's tree and visible non-day records.
    pub async fn day(
        &self,
        day_key: &DayKey,
    ) -> Result<Option<(ChronicleId, RecordTreeNode, Vec<DerivedRecord>)>, SillokError> {
        let Some(conn) = self.connect_existing().await? else {
            return Ok(None);
        };
        let Some(day_id) = day_id_by_key(&conn, day_key).await? else {
            return Ok(None);
        };
        let records = records_for_day(&conn, day_id).await?;
        let tree = build_tree(&records, day_id)?;
        let visible = records
            .into_iter()
            .filter(|record| record.record_id != day_id && record.status != RecordStatus::Retracted)
            .collect();
        Ok(Some((day_id, tree, visible)))
    }

    /// Queries visible current records by creation time and optional filters.
    pub async fn query_records(
        &self,
        from: Timestamp,
        to: Timestamp,
        context: Option<&str>,
        tag: Option<&str>,
        status: Option<RecordStatus>,
    ) -> Result<Vec<DerivedRecord>, SillokError> {
        let Some(conn) = self.connect_existing().await? else {
            return Ok(Vec::new());
        };
        query_records(&conn, from, to, context, tag, status).await
    }

    /// Reads all visible records for export.
    pub async fn visible_records(&self) -> Result<Vec<DerivedRecord>, SillokError> {
        let Some(conn) = self.connect_existing().await? else {
            return Ok(Vec::new());
        };
        load_records(
            &conn,
            &format!(
                "{RECORD_SELECT} WHERE r.record_status != ?1
                 ORDER BY r.record_created_at_ms, r.record_id"
            ),
            vec![Value::from(convert::status_label(RecordStatus::Retracted))],
        )
        .await
    }

    /// Reads a tree rooted at one record.
    pub async fn tree(&self, root_id: ChronicleId) -> Result<Option<RecordTreeNode>, SillokError> {
        let Some(conn) = self.connect_existing().await? else {
            return Ok(None);
        };
        let Some(root) = fetch_record(&conn, root_id).await? else {
            return Ok(None);
        };
        let records = records_for_day(&conn, root.day_id).await?;
        Ok(Some(build_tree(&records, root_id)?))
    }

    /// Validates the v2 database by replaying persisted events.
    pub async fn doctor(&self) -> Result<Option<StoreStats>, SillokError> {
        let Some(conn) = self.connect_existing().await? else {
            return Ok(None);
        };
        validate_integrity(&conn).await?;
        let archive = archive_from_db(&conn).await?;
        let view = ChronicleView::build(&archive)?;
        reducer::validate_parent_graph(&view)?;
        let stats = StoreStats {
            info: StoreInfo {
                archive_id: archive.archive_id,
                created_at: archive.created_at,
            },
            event_count: archive.events.len(),
            record_count: count_rows(&conn, "records").await?,
        };
        if stats.record_count != view.records.len() {
            return Err(SillokError::new(
                "projection_mismatch",
                format!(
                    "record projection has {} rows but event replay derives {} records",
                    stats.record_count,
                    view.records.len()
                ),
            ));
        }
        Ok(Some(stats))
    }

    /// Backs up and resets the v2 database.
    pub async fn truncate(
        &self,
        recorded_at: Timestamp,
        actor: String,
        context: WorkContext,
    ) -> Result<(StoreInfo, Option<PathBuf>), SillokError> {
        ensure_parent_dir(&self.path)?;
        let backup = if self.path.exists() {
            let backup_path = self.backup_path(recorded_at);
            fs::copy(&self.path, &backup_path)?;
            remove_db_file(&self.path)?;
            Some(backup_path)
        } else {
            None
        };
        let (info, _) = self.init(recorded_at, actor, context).await?;
        Ok((info, backup))
    }

    /// Imports a validated legacy archive into this v2 database path.
    pub async fn import_archive(&self, archive: &Archive) -> Result<StoreStats, SillokError> {
        if self.path.exists() {
            return Err(SillokError::new(
                "target_exists",
                format!("target store `{}` already exists", self.path.display()),
            ));
        }
        ensure_parent_dir(&self.path)?;
        let mut conn = self.connect().await?;
        crate::storage::sql::schema::create(&conn).await?;
        let tx = begin_write_transaction(&mut conn).await?;
        write_meta(
            &tx,
            "store_datashape_version",
            &STORE_DATASHAPE_VERSION.to_string(),
        )
        .await?;
        write_meta(&tx, "archive_id", &archive.archive_id.to_string()).await?;
        write_meta(
            &tx,
            "created_at_ms",
            &archive.created_at.as_millis().to_string(),
        )
        .await?;
        for event in &archive.events {
            insert_event(&tx, event).await?;
        }
        let view = ChronicleView::build(archive)?;
        for record in view.records.values() {
            upsert_record(&tx, record).await?;
        }
        tx.commit().await?;
        validate_integrity(&conn).await?;
        checkpoint(&conn).await?;
        Ok(StoreStats {
            info: StoreInfo {
                archive_id: archive.archive_id,
                created_at: archive.created_at,
            },
            event_count: archive.events.len(),
            record_count: view.records.len(),
        })
    }

    /// Atomically replaces the database with projections rebuilt from an archive.
    pub async fn rebuild_from_archive(
        &self,
        archive: &Archive,
        recorded_at: Timestamp,
    ) -> Result<(StoreStats, Option<PathBuf>), SillokError> {
        ChronicleView::build(archive)?;
        ensure_parent_dir(&self.path)?;
        let temp_path = self.temp_rebuild_path(recorded_at);
        let temp_store = Self::new(temp_path.clone());
        let stats = temp_store.import_archive(archive).await?;
        temp_store.doctor().await?;
        remove_db_sidecars(&temp_path)?;

        let backup = if self.path.exists() {
            if let Some(conn) = self.connect_existing().await? {
                checkpoint(&conn).await?;
                drop(conn);
            }
            let backup_path = self.rebuild_backup_path(recorded_at);
            fs::rename(&self.path, &backup_path)?;
            remove_db_sidecars(&self.path)?;
            Some(backup_path)
        } else {
            remove_db_sidecars(&self.path)?;
            None
        };

        match fs::rename(&temp_path, &self.path) {
            Ok(()) => Ok((stats, backup)),
            Err(error) => {
                if let Some(backup_path) = &backup {
                    match fs::rename(backup_path, &self.path) {
                        Ok(()) | Err(_) => {}
                    }
                }
                Err(error.into())
            }
        }
    }

    /// Records a task and returns its current derived state.
    pub async fn record_task(&self, input: TaskInput) -> Result<DerivedRecord, SillokError> {
        let mut conn = self.connect().await?;
        ensure_initialized(
            &mut conn,
            input.recorded_at,
            input.actor.clone(),
            input.context.clone(),
        )
        .await?;
        let tx = begin_write_transaction(&mut conn).await?;
        let (day_id, parent_id) = match input.parent {
            Some(parent_id) => {
                let parent = require_active_record(&tx, parent_id).await?;
                (parent.day_id, parent_id)
            }
            None => {
                let day_id = ensure_day(
                    &tx,
                    input.day_key,
                    input.event_at,
                    input.recorded_at,
                    &input.actor,
                    &input.context,
                )
                .await?;
                (day_id, day_id)
            }
        };
        let task_id = ChronicleId::new_v7();
        let event = ChronicleEvent::new(
            input.event_at,
            input.recorded_at,
            input.actor,
            input.context.clone(),
            EventKind::TaskRecorded {
                task_id,
                day_id,
                parent_id,
                text: input.text,
                purpose: input.purpose,
                tags: input.tags.clone(),
                status: input.status,
            },
        );
        insert_event(&tx, &event).await?;
        let record = DerivedRecord {
            record_id: task_id,
            kind: RecordKind::Task,
            day_id,
            parent_id: Some(parent_id),
            text: match &event.kind {
                EventKind::TaskRecorded { text, .. } => text.clone(),
                _ => String::new(),
            },
            purpose: match &event.kind {
                EventKind::TaskRecorded { purpose, .. } => purpose.clone(),
                _ => None,
            },
            tags: input.tags,
            status: input.status,
            created_at: input.event_at,
            updated_at: input.recorded_at,
            context: input.context,
            retraction_reason: None,
            day_key: None,
        };
        upsert_record(&tx, &record).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Adds an objective for a day.
    pub async fn add_objective(&self, input: ObjectiveInput) -> Result<DerivedRecord, SillokError> {
        let mut conn = self.connect().await?;
        ensure_initialized(
            &mut conn,
            input.recorded_at,
            input.actor.clone(),
            input.context.clone(),
        )
        .await?;
        let tx = begin_write_transaction(&mut conn).await?;
        let day_id = ensure_day(
            &tx,
            input.day_key,
            input.event_at,
            input.recorded_at,
            &input.actor,
            &input.context,
        )
        .await?;
        let objective_id = ChronicleId::new_v7();
        let event = ChronicleEvent::new(
            input.event_at,
            input.recorded_at,
            input.actor,
            input.context.clone(),
            EventKind::ObjectiveAdded {
                objective_id,
                day_id,
                text: input.text.clone(),
                tags: input.tags.clone(),
            },
        );
        insert_event(&tx, &event).await?;
        let record = DerivedRecord {
            record_id: objective_id,
            kind: RecordKind::Objective,
            day_id,
            parent_id: Some(day_id),
            text: input.text,
            purpose: None,
            tags: input.tags,
            status: RecordStatus::Open,
            created_at: input.event_at,
            updated_at: input.recorded_at,
            context: input.context,
            retraction_reason: None,
            day_key: None,
        };
        upsert_record(&tx, &record).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Marks an objective complete.
    pub async fn complete_objective(
        &self,
        input: CompleteObjectiveInput,
    ) -> Result<DerivedRecord, SillokError> {
        let mut conn = self.connect().await?;
        ensure_initialized(
            &mut conn,
            input.recorded_at,
            input.actor.clone(),
            input.context.clone(),
        )
        .await?;
        let tx = begin_write_transaction(&mut conn).await?;
        let mut record = require_active_record(&tx, input.objective_id).await?;
        if record.kind != RecordKind::Objective {
            tx.rollback().await?;
            return Err(SillokError::new(
                "invalid_record_kind",
                format!("record `{}` is not an objective", input.objective_id),
            ));
        }
        let event = ChronicleEvent::new(
            input.event_at,
            input.recorded_at,
            input.actor,
            input.context,
            EventKind::ObjectiveCompleted {
                objective_id: input.objective_id,
                note: input.note.clone(),
            },
        );
        insert_event(&tx, &event).await?;
        record.status = RecordStatus::Completed;
        record.updated_at = input.recorded_at;
        if input.note.is_some() {
            record.purpose = input.note;
        }
        upsert_record(&tx, &record).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Amends a record's current projection.
    pub async fn amend_record(&self, input: AmendInput) -> Result<DerivedRecord, SillokError> {
        let mut conn = self.connect().await?;
        ensure_initialized(
            &mut conn,
            input.recorded_at,
            input.actor.clone(),
            input.context.clone(),
        )
        .await?;
        let tx = begin_write_transaction(&mut conn).await?;
        let mut record = require_active_record(&tx, input.record_id).await?;
        let event = ChronicleEvent::new(
            input.event_at,
            input.recorded_at,
            input.actor,
            input.context,
            EventKind::TaskAmended {
                record_id: input.record_id,
                text: input.text.clone(),
                status: input.status,
                purpose: input.purpose.clone(),
                tags: input.tags.clone(),
            },
        );
        insert_event(&tx, &event).await?;
        if let Some(value) = input.text {
            record.text = value;
        }
        if let Some(value) = input.status {
            record.status = value;
        }
        if let Some(value) = input.purpose {
            record.purpose = Some(value);
        }
        if let Some(value) = input.tags {
            record.tags = value;
        }
        record.updated_at = input.recorded_at;
        upsert_record(&tx, &record).await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Retracts a record from visible views.
    pub async fn retract_record(&self, input: RetractInput) -> Result<DerivedRecord, SillokError> {
        let mut conn = self.connect().await?;
        ensure_initialized(
            &mut conn,
            input.recorded_at,
            input.actor.clone(),
            input.context.clone(),
        )
        .await?;
        let tx = begin_write_transaction(&mut conn).await?;
        let mut record = require_active_record(&tx, input.record_id).await?;
        if record.kind == RecordKind::Day {
            tx.rollback().await?;
            return Err(SillokError::new(
                "invalid_operation",
                "day records cannot be retracted; use truncate for a full reset",
            ));
        }
        let event = ChronicleEvent::new(
            input.event_at,
            input.recorded_at,
            input.actor,
            input.context,
            EventKind::TaskRetracted {
                record_id: input.record_id,
                reason: input.reason.clone(),
            },
        );
        insert_event(&tx, &event).await?;
        record.status = RecordStatus::Retracted;
        record.retraction_reason = Some(input.reason);
        record.updated_at = input.recorded_at;
        upsert_record(&tx, &record).await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn connect(&self) -> Result<Connection, SillokError> {
        ensure_parent_dir(&self.path)?;
        let db = Builder::new_local(&self.path.display().to_string())
            .build()
            .await?;
        let conn = db.connect()?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(conn)
    }

    async fn connect_existing(&self) -> Result<Option<Connection>, SillokError> {
        if self.path.exists() {
            Ok(Some(self.connect().await?))
        } else {
            Ok(None)
        }
    }

    fn backup_path(&self, timestamp: Timestamp) -> PathBuf {
        let mut path = self.path.clone();
        path.set_extension(format!("{}.bak.db", timestamp.as_millis()));
        path
    }

    fn temp_rebuild_path(&self, timestamp: Timestamp) -> PathBuf {
        let mut path = self.path.clone();
        path.set_extension(format!(
            "{}.{}.tmp.db",
            timestamp.as_millis(),
            ChronicleId::new_v7()
        ));
        path
    }

    fn rebuild_backup_path(&self, timestamp: Timestamp) -> PathBuf {
        let mut path = self.path.clone();
        path.set_extension(format!(
            "{}.{}.bak.db",
            timestamp.as_millis(),
            ChronicleId::new_v7()
        ));
        path
    }
}

/// Input for task creation.
pub struct TaskInput {
    pub recorded_at: Timestamp,
    pub event_at: Timestamp,
    pub actor: String,
    pub context: WorkContext,
    pub day_key: DayKey,
    pub parent: Option<ChronicleId>,
    pub text: String,
    pub purpose: Option<String>,
    pub tags: Vec<String>,
    pub status: RecordStatus,
}

/// Input for objective creation.
pub struct ObjectiveInput {
    pub recorded_at: Timestamp,
    pub event_at: Timestamp,
    pub actor: String,
    pub context: WorkContext,
    pub day_key: DayKey,
    pub text: String,
    pub tags: Vec<String>,
}

/// Input for objective completion.
pub struct CompleteObjectiveInput {
    pub recorded_at: Timestamp,
    pub event_at: Timestamp,
    pub actor: String,
    pub context: WorkContext,
    pub objective_id: ChronicleId,
    pub note: Option<String>,
}

/// Input for record amendments.
pub struct AmendInput {
    pub recorded_at: Timestamp,
    pub event_at: Timestamp,
    pub actor: String,
    pub context: WorkContext,
    pub record_id: ChronicleId,
    pub text: Option<String>,
    pub status: Option<RecordStatus>,
    pub purpose: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// Input for retractions.
pub struct RetractInput {
    pub recorded_at: Timestamp,
    pub event_at: Timestamp,
    pub actor: String,
    pub context: WorkContext,
    pub record_id: ChronicleId,
    pub reason: String,
}

const RECORD_SELECT: &str = "
    SELECT
        r.record_id,
        r.record_datashape_version,
        r.record_kind,
        r.record_day_id,
        r.record_parent_id,
        r.record_status,
        r.record_text,
        r.record_purpose,
        r.record_created_at_ms,
        r.record_updated_at_ms,
        r.record_retraction_reason,
        c.context_cwd,
        c.context_git_root,
        c.context_git_branch,
        c.context_git_head,
        c.context_git_remote,
        d.day_date,
        d.day_timezone
    FROM records r
    JOIN work_contexts c ON c.context_id = r.record_context_id
    LEFT JOIN days d ON d.day_id = r.record_id
";

const EVENT_SELECT: &str = "
    SELECT
        e.event_id,
        e.event_datashape_version,
        e.event_at_ms,
        e.event_recorded_at_ms,
        e.event_actor,
        e.event_payload,
        c.context_cwd,
        c.context_git_root,
        c.context_git_branch,
        c.context_git_head,
        c.context_git_remote
    FROM events e
    JOIN work_contexts c ON c.context_id = e.event_context_id
";

async fn fetch_record(
    conn: &Connection,
    record_id: ChronicleId,
) -> Result<Option<DerivedRecord>, SillokError> {
    let mut records = load_records(
        conn,
        &format!("{RECORD_SELECT} WHERE r.record_id = ?1"),
        vec![Value::from(convert::id_blob(record_id))],
    )
    .await?;
    Ok(records.pop())
}

async fn records_for_day(
    conn: &Connection,
    day_id: ChronicleId,
) -> Result<Vec<DerivedRecord>, SillokError> {
    load_records(
        conn,
        &format!(
            "{RECORD_SELECT} WHERE r.record_day_id = ?1
             ORDER BY r.record_created_at_ms, r.record_id"
        ),
        vec![Value::from(convert::id_blob(day_id))],
    )
    .await
}

async fn query_records(
    conn: &Connection,
    from: Timestamp,
    to: Timestamp,
    context: Option<&str>,
    tag: Option<&str>,
    status: Option<RecordStatus>,
) -> Result<Vec<DerivedRecord>, SillokError> {
    let mut sql = RECORD_SELECT.to_string();
    if tag.is_some() {
        sql.push_str(" JOIN record_tags rt ON rt.record_id = r.record_id");
    }
    sql.push_str(" WHERE r.record_created_at_ms >= ?1 AND r.record_created_at_ms <= ?2");
    sql.push_str(" AND r.record_status != ?3");
    let mut params = vec![
        Value::from(from.as_millis()),
        Value::from(to.as_millis()),
        Value::from(convert::status_label(RecordStatus::Retracted)),
    ];
    if let Some(required_status) = status {
        sql.push_str(" AND r.record_status = ?");
        params.push(Value::from(convert::status_label(required_status)));
    }
    if let Some(required_tag) = tag {
        sql.push_str(" AND rt.tag_text = ?");
        params.push(Value::from(required_tag.to_string()));
    }
    sql.push_str(" ORDER BY r.record_created_at_ms, r.record_id");
    let mut records = load_records(conn, &sql, params).await?;
    if let Some(required_context) = context {
        records.retain(|record| record.context.key_contains(required_context));
    }
    Ok(records)
}

async fn load_records(
    conn: &Connection,
    sql: &str,
    params: Vec<Value>,
) -> Result<Vec<DerivedRecord>, SillokError> {
    let mut rows = conn.query(sql, params_from_iter(params)).await?;
    let mut records = Vec::new();
    while let Some(row) = rows.next().await? {
        records.push(record_from_row(&row)?);
    }
    load_tags(conn, &mut records).await?;
    Ok(records)
}

fn record_from_row(row: &Row) -> Result<DerivedRecord, SillokError> {
    let version = row_i64(row, 1)?;
    if version != RECORD_DATASHAPE_VERSION {
        return Err(SillokError::new(
            "unsupported_datashape",
            format!("record datashape version `{version}` is not supported"),
        ));
    }
    let kind = convert::parse_kind(&row_text(row, 2)?)?;
    let day_date = row_opt_text(row, 16)?;
    let day_timezone = row_opt_text(row, 17)?;
    let day_key = match (day_date, day_timezone) {
        (Some(date), Some(timezone)) => Some(convert::day_key(date, timezone)),
        (None, None) => None,
        _ => {
            return Err(SillokError::new(
                "invalid_datashape",
                "day record has partial day key",
            ));
        }
    };
    Ok(DerivedRecord {
        record_id: row_id(row, 0)?,
        kind,
        day_id: row_id(row, 3)?,
        parent_id: row_opt_id(row, 4)?,
        status: convert::parse_status(&row_text(row, 5)?)?,
        text: row_text(row, 6)?,
        purpose: row_opt_text(row, 7)?,
        tags: Vec::new(),
        created_at: convert::timestamp(row_i64(row, 8)?),
        updated_at: convert::timestamp(row_i64(row, 9)?),
        retraction_reason: row_opt_text(row, 10)?,
        context: WorkContext {
            cwd: row_opt_text(row, 11)?,
            git_root: row_opt_text(row, 12)?,
            git_branch: row_opt_text(row, 13)?,
            git_head: row_opt_text(row, 14)?,
            git_remote: row_opt_text(row, 15)?,
        },
        day_key,
    })
}

async fn load_tags(conn: &Connection, records: &mut [DerivedRecord]) -> Result<(), SillokError> {
    if records.is_empty() {
        return Ok(());
    }
    let mut tags: HashMap<ChronicleId, Vec<String>> = HashMap::with_capacity(records.len());
    for chunk in records.chunks(500) {
        let placeholders = placeholders(chunk.len());
        let sql = format!(
            "SELECT record_id, tag_text FROM record_tags
             WHERE record_id IN ({placeholders})
             ORDER BY tag_text"
        );
        let params = chunk
            .iter()
            .map(|record| Value::from(convert::id_blob(record.record_id)))
            .collect::<Vec<_>>();
        let mut rows = conn.query(sql, params_from_iter(params)).await?;
        while let Some(row) = rows.next().await? {
            tags.entry(row_id(&row, 0)?)
                .or_default()
                .push(row_text(&row, 1)?);
        }
    }
    for record in records {
        if let Some(values) = tags.remove(&record.record_id) {
            record.tags = values;
        }
    }
    Ok(())
}

fn build_tree(
    records: &[DerivedRecord],
    root_id: ChronicleId,
) -> Result<RecordTreeNode, SillokError> {
    let mut by_id = HashMap::with_capacity(records.len());
    let mut children: HashMap<ChronicleId, Vec<ChronicleId>> = HashMap::new();
    for record in records {
        by_id.insert(record.record_id, record.clone());
        if let Some(parent_id) = record.parent_id {
            children
                .entry(parent_id)
                .or_default()
                .push(record.record_id);
        }
    }
    for bucket in children.values_mut() {
        bucket.sort_by_key(|id| {
            by_id
                .get(id)
                .map(|record| (record.created_at, record.record_id))
        });
    }
    tree_inner(root_id, &by_id, &children, &mut HashSet::new())
}

fn tree_inner(
    root_id: ChronicleId,
    by_id: &HashMap<ChronicleId, DerivedRecord>,
    children: &HashMap<ChronicleId, Vec<ChronicleId>>,
    visiting: &mut HashSet<ChronicleId>,
) -> Result<RecordTreeNode, SillokError> {
    let Some(root) = by_id.get(&root_id) else {
        return Err(SillokError::new(
            "record_not_found",
            format!("record `{root_id}` does not exist"),
        ));
    };
    if !visiting.insert(root_id) {
        return Err(SillokError::new(
            "parent_cycle",
            format!("cycle detected at `{root_id}`"),
        ));
    }
    let mut nodes = Vec::new();
    if let Some(child_ids) = children.get(&root_id) {
        for child_id in child_ids {
            let Some(child) = by_id.get(child_id) else {
                continue;
            };
            if child.status == RecordStatus::Retracted {
                continue;
            }
            nodes.push(tree_inner(*child_id, by_id, children, visiting)?);
        }
    }
    visiting.remove(&root_id);
    Ok(RecordTreeNode {
        record: root.clone(),
        children: nodes,
    })
}

async fn events_for_record(
    conn: &Connection,
    record_id: ChronicleId,
) -> Result<Vec<ChronicleEvent>, SillokError> {
    load_events(
        conn,
        &format!(
            "{EVENT_SELECT}
             JOIN event_refs er ON er.event_seq = e.event_seq
             WHERE er.ref_record_id = ?1
             ORDER BY e.event_recorded_at_ms, e.event_id"
        ),
        vec![Value::from(convert::id_blob(record_id))],
    )
    .await
}

async fn archive_from_db(conn: &Connection) -> Result<Archive, SillokError> {
    let info = read_info(conn).await?;
    let events = load_events(
        conn,
        &format!("{EVENT_SELECT} ORDER BY e.event_seq"),
        Vec::new(),
    )
    .await?;
    Ok(Archive {
        schema_version: ARCHIVE_SCHEMA_VERSION,
        archive_id: info.archive_id,
        created_at: info.created_at,
        events,
    })
}

async fn load_events(
    conn: &Connection,
    sql: &str,
    params: Vec<Value>,
) -> Result<Vec<ChronicleEvent>, SillokError> {
    let mut rows = conn.query(sql, params_from_iter(params)).await?;
    let mut events = Vec::new();
    while let Some(row) = rows.next().await? {
        let version = row_i64(&row, 1)?;
        if version != EVENT_DATASHAPE_VERSION {
            return Err(SillokError::new(
                "unsupported_datashape",
                format!("event datashape version `{version}` is not supported"),
            ));
        }
        events.push(ChronicleEvent {
            event_id: row_id(&row, 0)?,
            event_at: convert::timestamp(row_i64(&row, 2)?),
            recorded_at: convert::timestamp(row_i64(&row, 3)?),
            actor: row_text(&row, 4)?,
            kind: bitcode::decode::<EventKind>(&row_blob(&row, 5)?)?,
            context: WorkContext {
                cwd: row_opt_text(&row, 6)?,
                git_root: row_opt_text(&row, 7)?,
                git_branch: row_opt_text(&row, 8)?,
                git_head: row_opt_text(&row, 9)?,
                git_remote: row_opt_text(&row, 10)?,
            },
        });
    }
    Ok(events)
}

async fn count_rows(conn: &Connection, table: &'static str) -> Result<usize, SillokError> {
    let mut rows = conn
        .query(format!("SELECT COUNT(*) FROM {table}"), ())
        .await?;
    let row = require_row(&mut rows, "invalid_datashape").await?;
    let count = row_i64(&row, 0)?;
    usize::try_from(count).map_err(|error| SillokError::new("invalid_datashape", error.to_string()))
}

async fn validate_integrity(conn: &Connection) -> Result<(), SillokError> {
    validate_store_version(conn).await?;
    let mut rows = conn.query("PRAGMA integrity_check", ()).await?;
    let row = require_row(&mut rows, "integrity_check_failed").await?;
    let result = row_text(&row, 0)?;
    if result == "ok" {
        Ok(())
    } else {
        Err(SillokError::new(
            "integrity_check_failed",
            format!("database integrity check returned `{result}`"),
        ))
    }
}

async fn checkpoint(conn: &Connection) -> Result<(), SillokError> {
    let mut rows = conn.query("PRAGMA wal_checkpoint(TRUNCATE)", ()).await?;
    while rows.next().await?.is_some() {}
    conn.cacheflush()?;
    Ok(())
}

fn placeholders(count: usize) -> String {
    (1..=count)
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn remove_db_file(path: &Path) -> Result<(), SillokError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn remove_db_sidecars(path: &Path) -> Result<(), SillokError> {
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        remove_db_file(&PathBuf::from(sidecar))?;
    }
    Ok(())
}

async fn ensure_initialized(
    conn: &mut Connection,
    recorded_at: Timestamp,
    actor: String,
    context: WorkContext,
) -> Result<bool, SillokError> {
    crate::storage::sql::schema::create(conn).await?;
    if read_optional_meta(conn, "store_datashape_version")
        .await?
        .is_some()
    {
        validate_store_version(conn).await?;
        return Ok(false);
    }
    let tx = begin_write_transaction(conn).await?;
    let archive_id = ChronicleId::new_v7();
    write_meta(
        &tx,
        "store_datashape_version",
        &STORE_DATASHAPE_VERSION.to_string(),
    )
    .await?;
    write_meta(&tx, "archive_id", &archive_id.to_string()).await?;
    write_meta(&tx, "created_at_ms", &recorded_at.as_millis().to_string()).await?;
    let init = ChronicleEvent::new(
        recorded_at,
        recorded_at,
        actor,
        context,
        EventKind::ArchiveInitialized { archive_id },
    );
    insert_event(&tx, &init).await?;
    tx.commit().await?;
    Ok(true)
}

async fn begin_write_transaction(conn: &mut Connection) -> Result<Transaction<'_>, SillokError> {
    conn.transaction_with_behavior(TransactionBehavior::Immediate)
        .await
        .map_err(SillokError::from)
}

async fn ensure_day(
    conn: &Connection,
    day_key: DayKey,
    event_at: Timestamp,
    recorded_at: Timestamp,
    actor: &str,
    context: &WorkContext,
) -> Result<ChronicleId, SillokError> {
    if let Some(day_id) = day_id_by_key(conn, &day_key).await? {
        return Ok(day_id);
    }
    let day_id = ChronicleId::new_v7();
    let event = ChronicleEvent::new(
        event_at,
        recorded_at,
        actor.to_string(),
        context.clone(),
        EventKind::DayOpened {
            day_id,
            day_key: day_key.clone(),
        },
    );
    insert_event(conn, &event).await?;
    conn.execute(
        "INSERT OR IGNORE INTO days (day_id, day_date, day_timezone) VALUES (?1, ?2, ?3)",
        params![
            convert::id_blob(day_id),
            day_key.date.clone(),
            day_key.timezone.clone()
        ],
    )
    .await?;
    let record = DerivedRecord {
        record_id: day_id,
        kind: RecordKind::Day,
        day_id,
        parent_id: None,
        text: format!("Day {}", day_key.date),
        purpose: None,
        tags: Vec::new(),
        status: RecordStatus::Open,
        created_at: event_at,
        updated_at: recorded_at,
        context: context.clone(),
        retraction_reason: None,
        day_key: Some(day_key),
    };
    upsert_record(conn, &record).await?;
    Ok(day_id)
}

async fn day_id_by_key(
    conn: &Connection,
    day_key: &DayKey,
) -> Result<Option<ChronicleId>, SillokError> {
    let mut rows = conn
        .query(
            "SELECT day_id FROM days WHERE day_date = ?1 AND day_timezone = ?2",
            params![day_key.date.clone(), day_key.timezone.clone()],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok(Some(row_id(&row, 0)?)),
        None => Ok(None),
    }
}

async fn require_active_record(
    conn: &Connection,
    record_id: ChronicleId,
) -> Result<DerivedRecord, SillokError> {
    let record = match fetch_record(conn, record_id).await? {
        Some(value) => value,
        None => {
            return Err(SillokError::new(
                "record_not_found",
                format!("record `{record_id}` does not exist"),
            ));
        }
    };
    if record.status == RecordStatus::Retracted {
        Err(SillokError::new(
            "record_retracted",
            format!("record `{record_id}` has been retracted"),
        ))
    } else {
        Ok(record)
    }
}

async fn insert_event(conn: &Connection, event: &ChronicleEvent) -> Result<i64, SillokError> {
    let context_id = upsert_context(conn, &event.context).await?;
    let payload = bitcode::encode(&event.kind);
    let primary = event.primary_record_id().map(convert::id_blob);
    conn.execute(
        "INSERT INTO events (
            event_id, event_datashape_version, event_kind, event_primary_record_id,
            event_at_ms, event_recorded_at_ms, event_actor, event_context_id, event_payload
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            convert::id_blob(event.event_id),
            EVENT_DATASHAPE_VERSION,
            convert::event_kind_label(&event.kind),
            primary,
            event.event_at.as_millis(),
            event.recorded_at.as_millis(),
            event.actor.clone(),
            context_id,
            payload,
        ],
    )
    .await?;
    let event_seq = conn.last_insert_rowid();
    let mut seen = HashSet::new();
    for ref_id in event.kind.referenced_ids() {
        if !seen.insert(ref_id) {
            continue;
        }
        conn.execute(
            "INSERT OR IGNORE INTO event_refs (event_seq, ref_record_id, ref_role)
             VALUES (?1, ?2, ?3)",
            params![event_seq, convert::id_blob(ref_id), "ref"],
        )
        .await?;
    }
    Ok(event_seq)
}

async fn upsert_context(conn: &Connection, context: &WorkContext) -> Result<i64, SillokError> {
    let context_json = convert::context_json(context)?;
    conn.execute(
        "INSERT OR IGNORE INTO work_contexts (
            context_json, context_cwd, context_git_root, context_git_branch,
            context_git_head, context_git_remote
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            context_json.clone(),
            context.cwd.clone(),
            context.git_root.clone(),
            context.git_branch.clone(),
            context.git_head.clone(),
            context.git_remote.clone(),
        ],
    )
    .await?;
    let mut rows = conn
        .query(
            "SELECT context_id FROM work_contexts WHERE context_json = ?1",
            params![context_json],
        )
        .await?;
    let row = require_row(&mut rows, "context_not_found").await?;
    row_i64(&row, 0)
}

async fn upsert_record(conn: &Connection, record: &DerivedRecord) -> Result<(), SillokError> {
    if record.kind == RecordKind::Day
        && let Some(day_key) = &record.day_key
    {
        conn.execute(
            "INSERT OR IGNORE INTO days (day_id, day_date, day_timezone) VALUES (?1, ?2, ?3)",
            params![
                convert::id_blob(record.record_id),
                day_key.date.clone(),
                day_key.timezone.clone(),
            ],
        )
        .await?;
    }
    let context_id = upsert_context(conn, &record.context).await?;
    conn.execute(
        "INSERT OR REPLACE INTO records (
            record_id, record_datashape_version, record_kind, record_day_id,
            record_parent_id, record_status, record_text, record_purpose,
            record_created_at_ms, record_updated_at_ms, record_context_id,
            record_retraction_reason
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            convert::id_blob(record.record_id),
            RECORD_DATASHAPE_VERSION,
            convert::kind_label(record.kind),
            convert::id_blob(record.day_id),
            convert::optional_id_blob(record.parent_id),
            convert::status_label(record.status),
            record.text.clone(),
            record.purpose.clone(),
            record.created_at.as_millis(),
            record.updated_at.as_millis(),
            context_id,
            record.retraction_reason.clone(),
        ],
    )
    .await?;
    conn.execute(
        "DELETE FROM record_tags WHERE record_id = ?1",
        params![convert::id_blob(record.record_id)],
    )
    .await?;
    for tag in &record.tags {
        conn.execute(
            "INSERT INTO record_tags (record_id, tag_text) VALUES (?1, ?2)",
            params![convert::id_blob(record.record_id), tag.clone()],
        )
        .await?;
    }
    Ok(())
}

async fn read_info(conn: &Connection) -> Result<StoreInfo, SillokError> {
    validate_store_version(conn).await?;
    let archive_id = ChronicleId::parse(&read_required_meta(conn, "archive_id").await?)?;
    let created_at = read_required_meta(conn, "created_at_ms")
        .await?
        .parse::<i64>()
        .map(Timestamp::from_millis)
        .map_err(|error| SillokError::new("invalid_datashape", error.to_string()))?;
    Ok(StoreInfo {
        archive_id,
        created_at,
    })
}

async fn validate_store_version(conn: &Connection) -> Result<(), SillokError> {
    let version = read_required_meta(conn, "store_datashape_version").await?;
    if version == STORE_DATASHAPE_VERSION.to_string() {
        Ok(())
    } else {
        Err(SillokError::new(
            "unsupported_datashape",
            format!("store datashape version `{version}` is not supported"),
        ))
    }
}

async fn write_meta(conn: &Connection, key: &str, value: &str) -> Result<(), SillokError> {
    conn.execute(
        "INSERT OR REPLACE INTO sillok_meta (meta_key, meta_value) VALUES (?1, ?2)",
        params![key, value],
    )
    .await?;
    Ok(())
}

async fn read_optional_meta(conn: &Connection, key: &str) -> Result<Option<String>, SillokError> {
    let mut rows = conn
        .query(
            "SELECT meta_value FROM sillok_meta WHERE meta_key = ?1",
            params![key],
        )
        .await?;
    match rows.next().await? {
        Some(row) => Ok(Some(row_text(&row, 0)?)),
        None => Ok(None),
    }
}

async fn read_required_meta(conn: &Connection, key: &str) -> Result<String, SillokError> {
    match read_optional_meta(conn, key).await? {
        Some(value) => Ok(value),
        None => Err(SillokError::new(
            "invalid_datashape",
            format!("missing metadata key `{key}`"),
        )),
    }
}

fn ensure_parent_dir(path: &Path) -> Result<(), SillokError> {
    match path.parent() {
        Some(parent) => {
            fs::create_dir_all(parent)?;
            Ok(())
        }
        None => Err(SillokError::new(
            "store_path_error",
            format!("store path `{}` has no parent", path.display()),
        )),
    }
}

async fn require_row(rows: &mut turso::Rows, code: &'static str) -> Result<Row, SillokError> {
    match rows.next().await? {
        Some(row) => Ok(row),
        None => Err(SillokError::new(code, "query returned no rows")),
    }
}

fn row_text(row: &Row, idx: usize) -> Result<String, SillokError> {
    match row.get_value(idx)? {
        Value::Text(value) => Ok(value),
        value => Err(SillokError::new(
            "invalid_datashape",
            format!("expected text column, got {value:?}"),
        )),
    }
}

fn row_opt_text(row: &Row, idx: usize) -> Result<Option<String>, SillokError> {
    match row.get_value(idx)? {
        Value::Text(value) => Ok(Some(value)),
        Value::Null => Ok(None),
        value => Err(SillokError::new(
            "invalid_datashape",
            format!("expected nullable text column, got {value:?}"),
        )),
    }
}

fn row_i64(row: &Row, idx: usize) -> Result<i64, SillokError> {
    match row.get_value(idx)? {
        Value::Integer(value) => Ok(value),
        value => Err(SillokError::new(
            "invalid_datashape",
            format!("expected integer column, got {value:?}"),
        )),
    }
}

fn row_blob(row: &Row, idx: usize) -> Result<Vec<u8>, SillokError> {
    match row.get_value(idx)? {
        Value::Blob(value) => Ok(value),
        value => Err(SillokError::new(
            "invalid_datashape",
            format!("expected blob column, got {value:?}"),
        )),
    }
}

fn row_opt_id(row: &Row, idx: usize) -> Result<Option<ChronicleId>, SillokError> {
    match row.get_value(idx)? {
        Value::Blob(value) => Ok(Some(ChronicleId::from_slice(&value)?)),
        Value::Null => Ok(None),
        value => Err(SillokError::new(
            "invalid_datashape",
            format!("expected nullable id blob, got {value:?}"),
        )),
    }
}

fn row_id(row: &Row, idx: usize) -> Result<ChronicleId, SillokError> {
    ChronicleId::from_slice(&row_blob(row, idx)?)
}
