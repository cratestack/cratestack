//! Two services that share one `.cstack` file through `import` (multi-file
//! schemas, since 0.19.0; `docs/design/multi-file-schemas.md`).
//!
//! `billing` and `shipping` are separate schemas, each with its own root
//! file and its own SQLite database. Both import `shared/common.cstack` (the
//! `Money` type and the `OrderRecord` mixin), which imports
//! `shared/currency.cstack` (the `Currency` enum). Each service generates its
//! own copy of the shared declarations: the two `Currency` enums are
//! different Rust types with the same variants, and the two `Money` structs
//! are wire-compatible because they have the same shape. An import shares
//! the shape. Each importer owns the storage.
//!
//! Real services would be two crates. Here they are two modules of one
//! crate, so that one test can compare them. Every `include_*_schema!` call
//! emits a module named `cratestack_schema`, so each call sits in a module of
//! its own.

/// The billing service: `Invoice`, priced in a shared `Currency`.
pub mod billing {
    cratestack::include_embedded_schema!("billing/schema.cstack");

    use cratestack::{RusqliteError, RusqliteRuntime};
    use cratestack_rusqlite::ddl::create_table_sql;

    use self::cratestack_schema::Money;
    use self::cratestack_schema::models::Invoice;

    /// Opens billing's own in-memory database and creates its one table.
    pub fn open_in_memory() -> Result<RusqliteRuntime, RusqliteError> {
        let runtime = RusqliteRuntime::open_in_memory()?;
        runtime.with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&cratestack_schema::INVOICE_MODEL))?;
            Ok(())
        })?;
        Ok(runtime)
    }

    /// An invoice's total, as the shared `Money` shape.
    pub fn total(invoice: &Invoice) -> Money {
        Money {
            amountMinor: invoice.totalMinor,
            currency: invoice.currency,
        }
    }
}

/// The shipping service: `Shipment`, with its own `ShipmentStatus` and an
/// insured value in the shared `Currency`.
pub mod shipping {
    cratestack::include_embedded_schema!("shipping/schema.cstack");

    use cratestack::{RusqliteError, RusqliteRuntime};
    use cratestack_rusqlite::ddl::create_table_sql;

    use self::cratestack_schema::Money;
    use self::cratestack_schema::models::Shipment;

    /// Opens shipping's own in-memory database and creates its one table.
    pub fn open_in_memory() -> Result<RusqliteRuntime, RusqliteError> {
        let runtime = RusqliteRuntime::open_in_memory()?;
        runtime.with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&cratestack_schema::SHIPMENT_MODEL))?;
            Ok(())
        })?;
        Ok(runtime)
    }

    /// A shipment's insured value, as the shared `Money` shape.
    pub fn insured_value(shipment: &Shipment) -> Money {
        Money {
            amountMinor: shipment.insuredMinor,
            currency: shipment.insuredCurrency,
        }
    }
}
