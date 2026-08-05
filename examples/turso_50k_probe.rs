use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sillok::domain::event::{ChronicleEvent, EventKind, RecordStatus, WorkContext};
use sillok::domain::id::ChronicleId;
use sillok::domain::time::{DayKey, Timestamp};
use sillok::error::SillokError;
use sillok::storage::sql::schema::{
    EVENT_DATASHAPE_VERSION, RECORD_DATASHAPE_VERSION, STORE_DATASHAPE_VERSION, create,
};
use sillok::storage::sql::store::{SqlStore, TaskInput};
use turso::{Builder, Connection, params};

const ENTRY_COUNT: usize = 50_000;
const BASE_MILLIS: i64 = 1_715_000_000_000;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), SillokError> {
    let path = PathBuf::from("/tmp/sillok-50k-turso-probe/sillok.db");
    remove_existing_store(&path)?;

    let context = WorkContext {
        cwd: Some("/tmp/sillok-50k-turso-probe".to_string()),
        git_root: Some("/tmp/sillok-50k-turso-probe".to_string()),
        git_branch: Some("main".to_string()),
        git_head: Some("0000000000000000000000000000000000000000".to_string()),
        git_remote: Some("local".to_string()),
    };
    let actor = "probe".to_string();
    let recorded_at = Timestamp::from_millis(BASE_MILLIS);
    let archive_id = ChronicleId::new_v7();
    let day_id = ChronicleId::new_v7();
    let day_key = DayKey {
        date: "2024-05-04".to_string(),
        timezone: "UTC".to_string(),
    };

    let fixture_started = Instant::now();
    build_fixture(
        &path,
        archive_id,
        day_id,
        &day_key,
        &actor,
        &context,
        recorded_at,
    )
    .await?;
    let fixture = fixture_started.elapsed();
    let initial_bytes = store_bytes(&path)?;

    let store = SqlStore::new(path.clone());
    let append_started = Instant::now();
    let appended = store
        .record_task(TaskInput {
            recorded_at: Timestamp::from_millis(BASE_MILLIS + ENTRY_COUNT as i64 + 1),
            event_at: Timestamp::from_millis(BASE_MILLIS + ENTRY_COUNT as i64 + 1),
            actor: actor.clone(),
            context: context.clone(),
            day_key: day_key.clone(),
            parent: None,
            text: "single appended entry after 50k".to_string(),
            purpose: Some("single write measurement".to_string()),
            tags: vec!["probe".to_string(), "storage".to_string()],
            status: RecordStatus::Completed,
        })
        .await?;
    let append_one = append_started.elapsed();

    let day_started = Instant::now();
    let day = store.day(&day_key).await?;
    let day_read = day_started.elapsed();

    let query_started = Instant::now();
    let queried = store
        .query_records(
            Timestamp::from_millis(BASE_MILLIS),
            Timestamp::from_millis(BASE_MILLIS + ENTRY_COUNT as i64 + 2),
            None,
            Some("probe"),
            Some(RecordStatus::Completed),
        )
        .await?;
    let query = query_started.elapsed();

    let doctor_started = Instant::now();
    let doctor = store.doctor().await?;
    let doctor_read = doctor_started.elapsed();
    let final_bytes = store_bytes(&path)?;

    println!("path={}", path.display());
    println!("target_task_records={ENTRY_COUNT}");
    println!("fixture_store_bytes={initial_bytes}");
    println!(
        "fixture_store_mib={:.3}",
        initial_bytes as f64 / 1_048_576.0
    );
    println!(
        "bytes_per_task_record={:.2}",
        initial_bytes as f64 / ENTRY_COUNT as f64
    );
    println!("fixture_build_ms={:.3}", fixture.as_secs_f64() * 1_000.0);
    println!("append_one_ms={:.3}", append_one.as_secs_f64() * 1_000.0);
    println!("appended_task_id={}", appended.record_id);
    match day {
        Some((_day_id, _tree, records)) => {
            println!("day_records={}", records.len());
        }
        None => println!("day_records=0"),
    }
    println!("day_read_ms={:.3}", day_read.as_secs_f64() * 1_000.0);
    println!("query_records={}", queried.len());
    println!("query_ms={:.3}", query.as_secs_f64() * 1_000.0);
    match doctor {
        Some(value) => {
            println!("doctor_events={}", value.event_count);
            println!("doctor_records={}", value.record_count);
        }
        None => {
            println!("doctor_events=0");
            println!("doctor_records=0");
        }
    }
    println!("doctor_ms={:.3}", doctor_read.as_secs_f64() * 1_000.0);
    println!("final_store_bytes={final_bytes}");
    println!("final_store_mib={:.3}", final_bytes as f64 / 1_048_576.0);
    Ok(())
}

async fn build_fixture(
    path: &Path,
    archive_id: ChronicleId,
    day_id: ChronicleId,
    day_key: &DayKey,
    actor: &str,
    context: &WorkContext,
    recorded_at: Timestamp,
) -> Result<(), SillokError> {
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent)?,
        None => {
            return Err(SillokError::new(
                "store_path_error",
                format!("store path `{}` has no parent", path.display()),
            ));
        }
    }
    let db = Builder::new_local(&path.display().to_string())
        .build()
        .await?;
    let mut conn = db.connect()?;
    create(&conn).await?;
    let tx = conn.transaction().await?;
    tx.execute(
        "INSERT INTO sillok_meta (meta_key, meta_value) VALUES (?1, ?2)",
        params![
            "store_datashape_version",
            STORE_DATASHAPE_VERSION.to_string()
        ],
    )
    .await?;
    tx.execute(
        "INSERT INTO sillok_meta (meta_key, meta_value) VALUES (?1, ?2)",
        params!["archive_id", archive_id.to_string()],
    )
    .await?;
    tx.execute(
        "INSERT INTO sillok_meta (meta_key, meta_value) VALUES (?1, ?2)",
        params!["created_at_ms", recorded_at.as_millis().to_string()],
    )
    .await?;
    insert_context(&tx, context).await?;
    insert_day(&tx, day_id, day_key).await?;
    insert_initial_events(
        &tx,
        archive_id,
        day_id,
        day_key,
        actor,
        context,
        recorded_at,
    )
    .await?;
    insert_day_record(&tx, day_id, day_key, context, recorded_at).await?;
    insert_tasks(&tx, day_id, actor, context).await?;
    tx.commit().await?;
    checkpoint(&conn).await?;
    Ok(())
}

async fn insert_context(conn: &Connection, context: &WorkContext) -> Result<(), SillokError> {
    tx_execute(
        conn,
        "INSERT INTO work_contexts (
            context_id, context_json, context_cwd, context_git_root,
            context_git_branch, context_git_head, context_git_remote
        ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            serde_json::to_string(context)?,
            context.cwd.clone(),
            context.git_root.clone(),
            context.git_branch.clone(),
            context.git_head.clone(),
            context.git_remote.clone(),
        ],
    )
    .await
}

async fn insert_day(
    conn: &Connection,
    day_id: ChronicleId,
    day_key: &DayKey,
) -> Result<(), SillokError> {
    tx_execute(
        conn,
        "INSERT INTO days (day_id, day_date, day_timezone) VALUES (?1, ?2, ?3)",
        params![
            day_id.to_vec(),
            day_key.date.clone(),
            day_key.timezone.clone()
        ],
    )
    .await
}

async fn insert_initial_events(
    conn: &Connection,
    archive_id: ChronicleId,
    day_id: ChronicleId,
    day_key: &DayKey,
    actor: &str,
    context: &WorkContext,
    recorded_at: Timestamp,
) -> Result<(), SillokError> {
    let init = ChronicleEvent::new(
        recorded_at,
        recorded_at,
        actor.to_string(),
        context.clone(),
        EventKind::ArchiveInitialized { archive_id },
    );
    let day = ChronicleEvent::new(
        recorded_at,
        recorded_at,
        actor.to_string(),
        context.clone(),
        EventKind::DayOpened {
            day_id,
            day_key: day_key.clone(),
        },
    );
    insert_event(conn, 1, &init).await?;
    insert_event_ref(conn, 1, archive_id).await?;
    insert_event(conn, 2, &day).await?;
    insert_event_ref(conn, 2, day_id).await?;
    Ok(())
}

async fn insert_day_record(
    conn: &Connection,
    day_id: ChronicleId,
    day_key: &DayKey,
    context: &WorkContext,
    recorded_at: Timestamp,
) -> Result<(), SillokError> {
    insert_record(
        conn,
        day_id,
        "day",
        day_id,
        None,
        "open",
        &format!("Day {}", day_key.date),
        None,
        recorded_at,
        recorded_at,
        context,
        None,
    )
    .await
}

async fn insert_tasks(
    conn: &Connection,
    day_id: ChronicleId,
    actor: &str,
    context: &WorkContext,
) -> Result<(), SillokError> {
    for index in 0..ENTRY_COUNT {
        if index > 0 && index % 10_000 == 0 {
            eprintln!("inserted {index} fixture tasks");
        }
        let timestamp = Timestamp::from_millis(BASE_MILLIS + index as i64);
        let task_id = ChronicleId::new_v7();
        let text = format!("probe entry {index:05}");
        let purpose = Some("measure turso store footprint at 50k records".to_string());
        let tags = vec!["probe".to_string(), "storage".to_string()];
        let event = ChronicleEvent::new(
            timestamp,
            timestamp,
            actor.to_string(),
            context.clone(),
            EventKind::TaskRecorded {
                task_id,
                day_id,
                parent_id: day_id,
                text: text.clone(),
                purpose: purpose.clone(),
                tags: tags.clone(),
                status: RecordStatus::Completed,
            },
        );
        let event_seq = 3 + index as i64;
        insert_event(conn, event_seq, &event).await?;
        insert_event_ref(conn, event_seq, task_id).await?;
        insert_event_ref(conn, event_seq, day_id).await?;
        insert_record(
            conn,
            task_id,
            "task",
            day_id,
            Some(day_id),
            "completed",
            &text,
            purpose.as_deref(),
            timestamp,
            timestamp,
            context,
            None,
        )
        .await?;
        for tag in tags {
            tx_execute(
                conn,
                "INSERT INTO record_tags (record_id, tag_text) VALUES (?1, ?2)",
                params![task_id.to_vec(), tag],
            )
            .await?;
        }
    }
    Ok(())
}

async fn insert_event(
    conn: &Connection,
    event_seq: i64,
    event: &ChronicleEvent,
) -> Result<(), SillokError> {
    tx_execute(
        conn,
        "INSERT INTO events (
            event_seq, event_id, event_datashape_version, event_kind,
            event_primary_record_id, event_at_ms, event_recorded_at_ms,
            event_actor, event_context_id, event_payload
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)",
        params![
            event_seq,
            event.event_id.to_vec(),
            EVENT_DATASHAPE_VERSION,
            event_kind_label(&event.kind),
            event.primary_record_id().map(|id| id.to_vec()),
            event.event_at.as_millis(),
            event.recorded_at.as_millis(),
            event.actor.clone(),
            bitcode::encode(&event.kind),
        ],
    )
    .await
}

async fn insert_event_ref(
    conn: &Connection,
    event_seq: i64,
    ref_id: ChronicleId,
) -> Result<(), SillokError> {
    tx_execute(
        conn,
        "INSERT OR IGNORE INTO event_refs (event_seq, ref_record_id, ref_role)
         VALUES (?1, ?2, ?3)",
        params![event_seq, ref_id.to_vec(), "ref"],
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn insert_record(
    conn: &Connection,
    record_id: ChronicleId,
    kind: &str,
    day_id: ChronicleId,
    parent_id: Option<ChronicleId>,
    status: &str,
    text: &str,
    purpose: Option<&str>,
    created_at: Timestamp,
    updated_at: Timestamp,
    _context: &WorkContext,
    retraction_reason: Option<&str>,
) -> Result<(), SillokError> {
    tx_execute(
        conn,
        "INSERT INTO records (
            record_id, record_datashape_version, record_kind, record_day_id,
            record_parent_id, record_status, record_text, record_purpose,
            record_created_at_ms, record_updated_at_ms, record_context_id,
            record_retraction_reason
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11)",
        params![
            record_id.to_vec(),
            RECORD_DATASHAPE_VERSION,
            kind,
            day_id.to_vec(),
            parent_id.map(|id| id.to_vec()),
            status,
            text,
            purpose.map(str::to_string),
            created_at.as_millis(),
            updated_at.as_millis(),
            retraction_reason.map(str::to_string),
        ],
    )
    .await
}

async fn tx_execute(
    conn: &Connection,
    sql: &str,
    params: impl turso::IntoParams,
) -> Result<(), SillokError> {
    conn.execute(sql, params).await?;
    Ok(())
}

async fn checkpoint(conn: &Connection) -> Result<(), SillokError> {
    let mut rows = conn.query("PRAGMA wal_checkpoint(TRUNCATE)", ()).await?;
    while rows.next().await?.is_some() {}
    conn.cacheflush()?;
    Ok(())
}

fn event_kind_label(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::ArchiveInitialized { .. } => "archive_initialized",
        EventKind::DayOpened { .. } => "day_opened",
        EventKind::ObjectiveAdded { .. } => "objective_added",
        EventKind::ObjectiveCompleted { .. } => "objective_completed",
        EventKind::TaskRecorded { .. } => "task_recorded",
        EventKind::TaskAmended { .. } => "task_amended",
        EventKind::TaskRetracted { .. } => "task_retracted",
        EventKind::TaskLinked { .. } => "task_linked",
        EventKind::TaskUnlinked { .. } => "task_unlinked",
    }
}

fn remove_existing_store(path: &Path) -> Result<(), SillokError> {
    match path.parent() {
        Some(parent) => match fs::remove_dir_all(parent) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        },
        None => {
            return Err(SillokError::new(
                "store_path_error",
                format!("store path `{}` has no parent", path.display()),
            ));
        }
    }
    Ok(())
}

fn store_bytes(path: &Path) -> Result<u64, SillokError> {
    let Some(parent) = path.parent() else {
        return Err(SillokError::new(
            "store_path_error",
            format!("store path `{}` has no parent", path.display()),
        ));
    };
    let mut total = 0;
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            total += entry.metadata()?.len();
        }
    }
    Ok(total)
}
