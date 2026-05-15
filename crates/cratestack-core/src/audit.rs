//! Audit log primitives.
//!
//! The audit subsystem is split into a record format (here,
//! backend-agnostic) and a sink trait. The canonical store is a table
//! inside the same database as the mutation, written inside the same
//! transaction so audit events never drift from the data they describe.
//! Downstream fan-out (Kafka, Redis pubsub, HTTP webhook) goes through
//! an [`AuditSink`] implementation; the table itself remains the source
//! of truth for compliance review.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::CoolError;
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditOperation {
    Create,
    Update,
    Delete,
}

impl AuditOperation {
    pub const fn as_str(&self) -> &'static str {
        match self {
            AuditOperation::Create => "create",
            AuditOperation::Update => "update",
            AuditOperation::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AuditActor {
    /// Actor identifier — typically the user id from the auth context.
    /// Omit when the operation runs without an authenticated principal
    /// (system jobs, migrations).
    pub id: Option<String>,
    /// Free-form claims captured from the auth context at the time of
    /// the operation. Banks use this for role/scope replay during
    /// forensics.
    pub claims: BTreeMap<String, Value>,
    /// Source IP recorded by the transport layer, if available.
    pub ip: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event_id: uuid::Uuid,
    /// Schema name as declared in the `.cstack` file — lets you scope
    /// audit queries to a single service without inspecting model
    /// strings.
    pub schema_name: String,
    /// Model name as declared in the schema (e.g. `Account`, `Transfer`).
    pub model: String,
    pub operation: AuditOperation,
    pub primary_key: serde_json::Value,
    pub actor: AuditActor,
    /// Tenant identifier captured from `PrincipalContext.tenant.id`
    /// when present. Banks running multi-tenant clusters use this to
    /// scope per-tenant audit exports.
    pub tenant: Option<String>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    /// W3C `traceparent`-style request id, if the transport layer
    /// captured one. Useful for stitching audit rows to APM traces.
    pub request_id: Option<String>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
}

/// Pluggable audit sink. Implementations fan audit events out to
/// downstream systems (Kafka topics, Redis pubsub, HTTP webhooks, S3
/// buckets) for long-term retention or SIEM ingestion. The in-database
/// audit table written by `cratestack_sqlx` remains the canonical
/// record; sinks are best-effort projections.
#[async_trait::async_trait]
pub trait AuditSink: Send + Sync + 'static {
    async fn record(&self, event: &AuditEvent) -> Result<(), CoolError>;
}

/// Default sink that does nothing. The in-database audit table is
/// treated as authoritative; downstream consumers are added by
/// wrapping a different sink (or composing several).
#[derive(Debug, Clone, Default)]
pub struct NoopAuditSink;

#[async_trait::async_trait]
impl AuditSink for NoopAuditSink {
    async fn record(&self, _event: &AuditEvent) -> Result<(), CoolError> {
        Ok(())
    }
}

/// Fan an audit event out to multiple sinks. Errors from any
/// individual sink are aggregated into [`CoolError::Internal`] so a
/// single failing downstream does not silently swallow problems with
/// the others.
pub struct MulticastAuditSink {
    sinks: Vec<Arc<dyn AuditSink>>,
}

impl MulticastAuditSink {
    pub fn new(sinks: Vec<Arc<dyn AuditSink>>) -> Self {
        Self { sinks }
    }
}

#[async_trait::async_trait]
impl AuditSink for MulticastAuditSink {
    async fn record(&self, event: &AuditEvent) -> Result<(), CoolError> {
        let mut errors = Vec::new();
        for sink in &self.sinks {
            if let Err(error) = sink.record(event).await {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(CoolError::Internal(format!(
                "{} audit sink(s) failed: {}",
                errors.len(),
                errors
                    .iter()
                    .map(|e| e.detail().unwrap_or("(no detail)").to_owned())
                    .collect::<Vec<_>>()
                    .join("; "),
            )))
        }
    }
}

/// Transaction isolation level requested by a procedure via
/// `@isolation(...)`. Mirrors the PostgreSQL spec: lower variants
/// tolerate more anomalies, higher ones cost more under contention.
/// Banks running multi-row updates (transfers, postings) typically
/// pick `Serializable` and pair it with retry-on-serialization-failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionIsolation {
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

impl TransactionIsolation {
    pub fn parse(value: &str) -> Result<Self, CoolError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "read_committed" | "read committed" => Ok(Self::ReadCommitted),
            "repeatable_read" | "repeatable read" => Ok(Self::RepeatableRead),
            "serializable" => Ok(Self::Serializable),
            other => Err(CoolError::Validation(format!(
                "unknown transaction isolation level '{other}'; expected one of \
                 'read_committed', 'repeatable_read', 'serializable'",
            ))),
        }
    }

    pub const fn as_sql(&self) -> &'static str {
        match self {
            Self::ReadCommitted => "READ COMMITTED",
            Self::RepeatableRead => "REPEATABLE READ",
            Self::Serializable => "SERIALIZABLE",
        }
    }
}
