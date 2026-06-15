//! Dev-only harness: boots the Studio router on 127.0.0.1:7878 with an
//! in-memory, multi-target SQLite workspace so the Leptos UI (served by
//! `trunk serve` on :8080, which proxies /api here) has real data to
//! render. Not shipped — purely for local UI work.
//!
//! Run with:
//!   cargo run -p cratestack-studio --no-default-features --example dev_server

use std::path::PathBuf;
use std::sync::Arc;

use cratestack_studio::audit::AuditLog;
use cratestack_studio::config::{TargetMode, WorkspaceConfig};
use cratestack_studio::data::api::ApiSource;
use cratestack_studio::data::sqlite::SqliteSource;
use cratestack_studio::workspace::{LoadedTarget, LoadedWorkspace};

const CATALOG_SCHEMA: &str = r#"
model Customer {
  id Int @id
  email String
  name String
  tier String
  posts Post[] @relation(fields: [id], references: [authorId])
}

model Post {
  id String @id
  authorId Int
  title String
  status String
  views Int
  author Customer @relation(fields: [authorId], references: [id])
}

model Product {
  id String @id
  name String
  price Int
  inStock Boolean
}
"#;

fn catalog_conn() -> rusqlite::Connection {
    let conn = rusqlite::Connection::open_in_memory().expect("sqlite open");
    conn.execute_batch(
        r#"
        CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL, name TEXT NOT NULL, tier TEXT NOT NULL);
        INSERT INTO customers VALUES
          (1, 'alice@example.com', 'Alice Mwangi', 'pro'),
          (2, 'bob@example.com',   'Bob Ngassa',   'free'),
          (3, 'carol@example.com', 'Carol Diallo', 'enterprise'),
          (4, 'dan@example.com',   'Dan Owusu',    'free'),
          (5, 'eve@example.com',   'Eve Santos',   'pro');

        CREATE TABLE posts (
          id TEXT PRIMARY KEY,
          author_id INTEGER NOT NULL,
          title TEXT NOT NULL,
          status TEXT NOT NULL,
          views INTEGER NOT NULL
        );
        INSERT INTO posts VALUES
          ('p1', 1, 'Shipping schema-first Rust',  'published', 1240),
          ('p2', 1, 'Why we forbid unsafe',        'draft',     0),
          ('p3', 2, 'A tour of the three macros',  'published', 880),
          ('p4', 3, 'Banking-grade policies',      'published', 2310),
          ('p5', 5, 'RPC vs REST transports',      'review',    42);

        CREATE TABLE products (
          id TEXT PRIMARY KEY,
          name TEXT NOT NULL,
          price INTEGER NOT NULL,
          in_stock INTEGER NOT NULL
        );
        INSERT INTO products VALUES
          ('sku-1', 'Starter plan',    0,    1),
          ('sku-2', 'Pro plan',        2900, 1),
          ('sku-3', 'Enterprise plan', 0,    0);
        "#,
    )
    .expect("ddl");
    conn
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let schema = Arc::new(cratestack_parser::parse_schema(CATALOG_SCHEMA).expect("schema parses"));

    let catalog = LoadedTarget {
        key: "catalog".to_owned(),
        display_name: "Catalog".to_owned(),
        mode: TargetMode::Rw,
        schema: schema.clone(),
        schema_path: PathBuf::from("schemas/catalog.cstack"),
        source: Arc::new(SqliteSource::new(catalog_conn(), schema.clone())),
        has_db: true,
        has_api: false,
    };

    let analytics = LoadedTarget {
        key: "analytics".to_owned(),
        display_name: "Analytics (read-only)".to_owned(),
        mode: TargetMode::Ro,
        schema: schema.clone(),
        schema_path: PathBuf::from("schemas/analytics.cstack"),
        source: Arc::new(SqliteSource::new(catalog_conn(), schema.clone())),
        has_db: true,
        has_api: false,
    };

    let upstream = LoadedTarget {
        key: "upstream-api".to_owned(),
        display_name: "Upstream API".to_owned(),
        mode: TargetMode::Ro,
        schema: schema.clone(),
        schema_path: PathBuf::from("schemas/catalog.cstack"),
        source: Arc::new(
            ApiSource::new("https://catalog.internal".to_owned(), None, schema.clone())
                .expect("ApiSource builds"),
        ),
        has_db: false,
        has_api: true,
    };

    let workspace = Arc::new(LoadedWorkspace {
        config: WorkspaceConfig {
            name: "acme-platform".to_owned(),
            default_mode: TargetMode::Ro,
            cors_dev: true,
        },
        targets: vec![Arc::new(catalog), Arc::new(analytics), Arc::new(upstream)],
        audit: Arc::new(AuditLog::new()),
    });

    let app = cratestack_studio::server::build_router(workspace);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:7878")
        .await
        .expect("bind 7878");
    println!("dev studio API on http://127.0.0.1:7878  (trunk serve UI on :8080)");
    axum::serve(listener, app).await.expect("serve");
}
