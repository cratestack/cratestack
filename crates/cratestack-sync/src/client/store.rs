//! Typed access to the client bookkeeping tables.

use chrono::{DateTime, Utc};
use cratestack_rusqlite::rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

use super::ClientError;
use crate::protocol::{Cursor, Mutation, RejectCode};

pub(crate) const CLIENT_ID: &str = "client_id";
pub(crate) const CURSOR: &str = "cursor";
pub(crate) const NEXT_SEQ: &str = "next_seq";
/// Highest seq the server reported consuming in a push response.
pub(crate) const PUSHED_SEQ: &str = "pushed_seq";
/// Highest seq acknowledged through the change stream.
pub(crate) const ACK_SEQ: &str = "ack_seq";
pub(crate) const MUTATION_TTL: &str = "mutation_ttl_secs";
/// Set when the next pull from zero must replace local state wholesale.
pub(crate) const RESYNC: &str = "resync";

pub(crate) fn get(conn: &Connection, key: &str) -> Result<Option<String>, ClientError> {
    Ok(conn
        .query_row(
            "SELECT value FROM cratestack_sync_meta WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()?)
}

pub(crate) fn set(conn: &Connection, key: &str, value: impl ToString) -> Result<(), ClientError> {
    conn.execute(
        "INSERT INTO cratestack_sync_meta (key, value) VALUES (?1, ?2) \
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value.to_string()],
    )?;
    Ok(())
}

pub(crate) fn clear(conn: &Connection, key: &str) -> Result<(), ClientError> {
    conn.execute("DELETE FROM cratestack_sync_meta WHERE key = ?1", [key])?;
    Ok(())
}

pub(crate) fn get_u64(conn: &Connection, key: &str) -> Result<u64, ClientError> {
    Ok(get(conn, key)?.and_then(|v| v.parse().ok()).unwrap_or(0))
}

pub(crate) fn client_id(conn: &Connection) -> Result<Option<Uuid>, ClientError> {
    Ok(get(conn, CLIENT_ID)?.and_then(|v| v.parse().ok()))
}

pub(crate) fn cursor(conn: &Connection) -> Result<Cursor, ClientError> {
    Ok(get(conn, CURSOR)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(Cursor::ZERO))
}

pub(crate) fn pending(conn: &Connection, after_seq: u64) -> Result<Vec<Mutation>, ClientError> {
    let mut stmt = conn.prepare(
        "SELECT seq, name, args, created_at FROM cratestack_sync_pending \
         WHERE seq > ?1 ORDER BY seq",
    )?;
    let rows = stmt.query_map([after_seq as i64], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (seq, name, args, created_at) = row?;
        out.push(Mutation {
            seq: seq as u64,
            name,
            args: serde_json::from_str(&args)?,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map_err(|e| ClientError::Protocol(e.to_string()))?
                .with_timezone(&Utc),
        });
    }
    Ok(out)
}

/// A mutation the server (or the local TTL) refused, kept for the user.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RejectedMutation {
    pub seq: u64,
    pub name: String,
    pub args: serde_json::Value,
    pub code: RejectCode,
    pub message: String,
}

/// Move one pending mutation to the rejected list.
pub(crate) fn reject(
    conn: &Connection,
    seq: u64,
    code: RejectCode,
    message: &str,
) -> Result<(), ClientError> {
    let code = serde_json::to_value(code)?;
    conn.execute(
        "INSERT INTO cratestack_sync_rejected \
           (seq, name, args, created_at, code, message, rejected_at) \
         SELECT seq, name, args, created_at, ?2, ?3, ?4 \
         FROM cratestack_sync_pending WHERE seq = ?1",
        params![
            seq as i64,
            code.as_str().unwrap_or("invalid"),
            message,
            Utc::now().to_rfc3339()
        ],
    )?;
    conn.execute(
        "DELETE FROM cratestack_sync_pending WHERE seq = ?1",
        [seq as i64],
    )?;
    Ok(())
}

pub(crate) fn rejected(conn: &Connection) -> Result<Vec<RejectedMutation>, ClientError> {
    let mut stmt = conn.prepare(
        "SELECT seq, name, args, code, message FROM cratestack_sync_rejected ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (seq, name, args, code, message) = row?;
        out.push(RejectedMutation {
            seq: seq as u64,
            name,
            args: serde_json::from_str(&args)?,
            code: serde_json::from_value(serde_json::Value::String(code))?,
            message,
        });
    }
    Ok(out)
}
