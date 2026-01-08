use rusqlite::{Connection, params};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpStatus {
    Pending,
    Sent,
    Acked,
    Failed,
}

impl OpStatus {
    fn to_i64(self) -> i64 {
        match self {
            OpStatus::Pending => 0,
            OpStatus::Sent => 1,
            OpStatus::Acked => 2,
            OpStatus::Failed => 3,
        }
    }

    fn from_i64(v: i64) -> Self {
        match v {
            0 => OpStatus::Pending,
            1 => OpStatus::Sent,
            2 => OpStatus::Acked,
            3 => OpStatus::Failed,
            _ => OpStatus::Failed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Op {
    pub op_id: Uuid,
    pub actor_id: String,
    pub kind: String,
    pub entity: String,
    pub payload_json: String,
    pub created_at_ms: u64,
    pub status: OpStatus,
}

impl Op {
    pub fn new_fake_upsert_note(actor_id: &str) -> Self {
        let now_ms = now_ms();
        let payload = serde_json::json!({
            "note_id": Uuid::new_v4().to_string(),
            "title": "Hola",
            "body": "Nota fake para demo",
            "updated_at_ms": now_ms
        });

        Self {
            op_id: Uuid::new_v4(),
            actor_id: actor_id.to_string(),
            kind: "UpsertNote".to_string(),
            entity: "note".to_string(),
            payload_json: payload.to_string(),
            created_at_ms: now_ms,
            status: OpStatus::Pending,
        }
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock went backwards");
    dur.as_millis() as u64
}

/// Done: al iniciar, crea DB si no existe (tabla e índices)
pub fn ensure_db(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;

        CREATE TABLE IF NOT EXISTS outbox (
            op_id          TEXT PRIMARY KEY,
            actor_id       TEXT NOT NULL,
            kind           TEXT NOT NULL,
            entity         TEXT NOT NULL,
            payload_json   TEXT NOT NULL,
            created_at_ms  INTEGER NOT NULL,
            status         INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_outbox_status ON outbox(status);
        CREATE INDEX IF NOT EXISTS idx_outbox_created_at ON outbox(created_at_ms);
        "#,
    )?;
    Ok(())
}

/// Función outbox_insert(op: Op) -> ok/error (insert real)
pub fn outbox_insert(conn: &Connection, op: &Op) -> Result<(), String> {
    conn.execute(
        r#"
        INSERT INTO outbox (op_id, actor_id, kind, entity, payload_json, created_at_ms, status)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            op.op_id.to_string(),
            op.actor_id,
            op.kind,
            op.entity,
            op.payload_json,
            op.created_at_ms as i64,
            op.status.to_i64(),
        ],
    )
    .map_err(|e| format!("outbox_insert error: {e}"))?;

    Ok(())
}

/// Función outbox_list_pending(limit) -> Vec<Op>
pub fn outbox_list_pending(conn: &Connection, limit: u32) -> Result<Vec<Op>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT op_id, actor_id, kind, entity, payload_json, created_at_ms, status
            FROM outbox
            WHERE status = ?1
            ORDER BY created_at_ms ASC
            LIMIT ?2
            "#,
        )
        .map_err(|e| format!("prepare error: {e}"))?;

    let rows = stmt
        .query_map(params![OpStatus::Pending.to_i64(), limit as i64], |row| {
            let op_id_str: String = row.get(0)?;
            let created_at_ms_i64: i64 = row.get(5)?;
            let status_i64: i64 = row.get(6)?;

            Ok(Op {
                op_id: Uuid::parse_str(&op_id_str).unwrap_or_else(|_| Uuid::nil()),
                actor_id: row.get(1)?,
                kind: row.get(2)?,
                entity: row.get(3)?,
                payload_json: row.get(4)?,
                created_at_ms: created_at_ms_i64.max(0) as u64,
                status: OpStatus::from_i64(status_i64),
            })
        })
        .map_err(|e| format!("query_map error: {e}"))?;

    let mut ops = Vec::new();
    for r in rows {
        ops.push(r.map_err(|e| format!("row error: {e}"))?);
    }

    Ok(ops)
}
