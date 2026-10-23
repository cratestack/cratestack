//! cratestack#1209: the embedded runtime fills `@default(uuid())`,
//! `@default(cuid())` and `@default(now())` on create. `create_table_sql`
//! writes no `DEFAULT` clauses, so the database cannot.

use chrono::Utc;
use cratestack::RusqliteRuntime;
use cratestack::include_embedded_schema;
use cratestack_core::BatchItemStatus;
use cratestack_core::error::parse_cuid;
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};
use uuid::Uuid;

include_embedded_schema!("tests/fixtures/generated_defaults.cstack");

use cratestack_schema::models::Ticket;
use cratestack_schema::{CreateTicketInput, TICKET_MODEL};

fn setup() -> RusqliteRuntime {
    let runtime = RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&TICKET_MODEL))
                .expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

fn input(title: &str) -> CreateTicketInput {
    CreateTicketInput {
        title: title.to_owned(),
    }
}

fn assert_generated(ticket: &Ticket) {
    assert!(parse_cuid(&ticket.code).is_ok(), "{}", ticket.code);
    assert_eq!(ticket.code.len(), 24);
    Uuid::parse_str(&ticket.reference).expect("reference is a uuid");
}

#[test]
fn create_fills_every_generated_column() {
    let runtime = setup();
    let tickets = ModelDelegate::<Ticket, Uuid>::new(&runtime, &TICKET_MODEL);

    let before = Utc::now();
    let first = tickets.create(input("first")).run().expect("create");
    let second = tickets.create(input("second")).run().expect("create");
    let after = Utc::now();

    for ticket in [&first, &second] {
        assert_generated(ticket);
        assert!(before <= ticket.createdAt && ticket.createdAt <= after);
    }
    assert_ne!(first.id, second.id);
    assert_ne!(first.code, second.code);
    assert_ne!(first.reference, second.reference);

    let reread = tickets
        .find_unique(first.id)
        .run()
        .expect("find")
        .expect("row");
    assert_eq!(reread.code, first.code);
    assert_eq!(reread.createdAt, first.createdAt);
}

#[test]
fn batch_create_fills_every_generated_column() {
    let runtime = setup();
    let tickets = ModelDelegate::<Ticket, Uuid>::new(&runtime, &TICKET_MODEL);

    let created = tickets
        .batch_create(vec![input("a"), input("b")])
        .run()
        .expect("batch_create");
    assert_eq!((created.summary.ok, created.summary.err), (2, 0));
    let ids: Vec<Uuid> = created
        .results
        .iter()
        .map(|item| match &item.status {
            BatchItemStatus::Ok { value } => {
                assert_generated(value);
                value.id
            }
            BatchItemStatus::Error { error } => panic!("{error:?}"),
        })
        .collect();
    assert_ne!(ids[0], ids[1]);
}
