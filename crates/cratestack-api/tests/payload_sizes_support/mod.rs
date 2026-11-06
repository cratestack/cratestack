//! The gate's bodies: fixed, realistic values of the generated types.
//! Nothing here is random at run time: UUIDs and timestamps are constants,
//! and the 500 list rows come from a seeded generator, so the encoded bytes,
//! and therefore the baseline, are the same on every machine.

use cratestack::chrono::{DateTime, Utc};
use cratestack::uuid::Uuid;

use crate::cratestack_schema::procedures::{create_product, list_events, transfer_funds};
use crate::cratestack_schema::{
    CreateProductInput, Event, EventType, Product, Transaction, TransferInput, TxStatus,
};

pub struct Bodies {
    pub create_product_args: create_product::Args,
    pub product: Product,
    pub transfer_args: transfer_funds::Args,
    pub transaction: Transaction,
    pub list_args: list_events::Args,
    pub events: Vec<Event>,
}

/// 2026-11-06T09:14:22.381204Z: microseconds, as a Postgres `TIMESTAMPTZ`
/// round-trips them.
fn at(offset_seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(1_793_956_462 + offset_seconds, 381_204_000).expect("in range")
}

fn decimal<D: std::str::FromStr>(text: &str) -> D
where
    D::Err: std::fmt::Debug,
{
    text.parse().expect("a valid decimal literal")
}

/// A 64-bit LCG (Knuth's MMIX constants): deterministic and dependency-free.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn uuid(&mut self) -> Uuid {
        let high = (self.next() << 31) ^ self.next();
        let low = (self.next() << 31) ^ self.next();
        Uuid::from_u64_pair(high, low)
    }
}

const EVENT_TYPES: [EventType; 5] = [
    EventType::Created,
    EventType::Updated,
    EventType::Settled,
    EventType::Refunded,
    EventType::Disputed,
];
const STATUSES: [TxStatus; 3] = [TxStatus::Pending, TxStatus::Settled, TxStatus::Rejected];

fn input() -> CreateProductInput {
    CreateProductInput {
        name: "Organic Arabica Coffee Beans 1kg".to_owned(),
        sku: "COF-ARB-1000".to_owned(),
        description: Some("Single-origin, medium roast.".to_owned()),
        priceAmount: decimal("24.90"),
        currency: "EUR".to_owned(),
        categoryId: Uuid::from_u128(0x6f1c_2a4e_8b3d_4c7a_9e21_0d5b_7f3a_1c88),
        stockQuantity: 120,
        isActive: true,
        tags: vec!["coffee".to_owned(), "organic".to_owned()],
    }
}

pub fn bodies() -> Bodies {
    let input = input();
    let product = Product {
        id: Uuid::from_u128(0x0b7e_4d21_95a3_4f60_8c1e_2a9d_6b40_e731),
        name: input.name.clone(),
        sku: input.sku.clone(),
        description: input.description.clone(),
        priceAmount: decimal("24.90"),
        currency: input.currency.clone(),
        categoryId: input.categoryId,
        stockQuantity: input.stockQuantity,
        isActive: input.isActive,
        tags: input.tags.clone(),
        createdAt: at(0),
        updatedAt: at(0),
        version: 1,
    };
    let transfer = TransferInput {
        sourceAccountId: Uuid::from_u128(0x3c90_5e17_a2b4_4d8f_b061_7e2c_94d3_0a5f),
        destinationAccountId: Uuid::from_u128(0x8a2f_61c3_0d49_4b7e_a5f8_13c6_e07b_92d4),
        amount: decimal("150.00"),
        currency: "EUR".to_owned(),
        reference: Some("Rent November".to_owned()),
    };
    let transaction = Transaction {
        transactionId: Uuid::from_u128(0xd41e_9b06_37c2_4a85_9f13_6e0a_b8c7_25f1),
        status: TxStatus::Settled,
        sourceAccountId: transfer.sourceAccountId,
        destinationAccountId: transfer.destinationAccountId,
        amount: decimal("150.00"),
        currency: transfer.currency.clone(),
        fee: decimal("0.00"),
        balanceAfter: decimal("2349.10"),
        reference: transfer.reference.clone(),
        createdAt: at(0),
        settledAt: None,
    };
    let mut lcg = Lcg(0x5eed);
    let events = (0..500)
        .map(|i| Event {
            id: lcg.uuid(),
            eventType: EVENT_TYPES[(lcg.next() % 5) as usize],
            status: STATUSES[(lcg.next() % 3) as usize],
            createdAt: at(i * 173 + (lcg.next() % 60) as i64),
            amount: decimal(&format!("{}.{:02}", lcg.next() % 100_000, lcg.next() % 100)),
        })
        .collect();
    Bodies {
        create_product_args: create_product::Args { input },
        product,
        transfer_args: transfer_funds::Args { input: transfer },
        transaction,
        list_args: list_events::Args { limit: Some(500) },
        events,
    }
}
