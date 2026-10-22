//! Two SQLite clients and a Postgres server, wired directly (no HTTP),
//! through the scenario the prototype exists for: offline writes on one
//! device, concurrent writes on another, a server-side write, convergence.

mod support;

use cratestack_rusqlite::rusqlite::{Connection, params};
use cratestack_sqlx::sqlx;
use cratestack_sync::SyncPolicy;
use cratestack_sync::client::{ClientError, SyncEngine, SyncedModel};
use cratestack_sync::server::SyncServer;
use serde_json::{Value, json};
use support::fixture::{ctx, server};
use support::pg::{connect_or_skip, serial_guard};

struct LocalTodos;

impl SyncedModel for LocalTodos {
    fn model(&self) -> &str {
        "Todo"
    }
    fn table(&self) -> &str {
        "todos"
    }
    fn pk_column(&self) -> &str {
        "id"
    }
    fn upsert(&self, conn: &Connection, row: &Value) -> Result<(), ClientError> {
        conn.execute(
            "INSERT INTO todos (id, title) VALUES (?1, ?2) \
             ON CONFLICT (id) DO UPDATE SET title = excluded.title",
            params![row["id"].as_str(), row["title"].as_str()],
        )?;
        Ok(())
    }
    fn delete(&self, conn: &Connection, pk: &str) -> Result<(), ClientError> {
        conn.execute("DELETE FROM todos WHERE id = ?1", [pk])?;
        Ok(())
    }
}

struct Device {
    conn: Connection,
    engine: SyncEngine,
    user: &'static str,
}

impl Device {
    async fn new(server: &SyncServer, user: &'static str) -> Self {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE todos (id TEXT PRIMARY KEY, title TEXT NOT NULL)")
            .unwrap();
        let engine = SyncEngine::new()
            .model(LocalTodos)
            .mutation("todo.create", |c: &Connection, a: &Value| {
                c.execute(
                    "INSERT INTO todos (id, title) VALUES (?1, ?2)",
                    params![a["id"].as_str(), a["title"].as_str()],
                )?;
                Ok(())
            })
            .mutation("todo.rename", |c: &Connection, a: &Value| {
                c.execute(
                    "UPDATE todos SET title = ?2 WHERE id = ?1",
                    params![a["id"].as_str(), a["title"].as_str()],
                )?;
                Ok(())
            });
        engine.install(&mut conn).unwrap();
        let registered = server
            .register(&ctx(user), engine.register_request(None))
            .await
            .unwrap();
        engine.apply_register(&mut conn, registered).unwrap();
        Self { conn, engine, user }
    }

    fn write(&mut self, name: &str, args: Value) {
        self.engine.mutate(&mut self.conn, name, args).unwrap();
    }

    /// One full round: push everything, then pull until caught up.
    async fn sync(&mut self, server: &SyncServer) {
        while let Some(request) = self.engine.push_request(&self.conn, 50).unwrap() {
            let response = server.push(&ctx(self.user), request).await.unwrap();
            self.engine.apply_push(&mut self.conn, response).unwrap();
        }
        loop {
            let request = self.engine.pull_request(&self.conn).unwrap().unwrap();
            let response = server.pull(&ctx(self.user), request).await.unwrap();
            if !self
                .engine
                .apply_pull(&mut self.conn, response)
                .unwrap()
                .has_more
            {
                break;
            }
        }
    }

    fn titles(&self) -> Vec<(String, String)> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title FROM todos ORDER BY id")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
}

fn pair(id: &str, title: &str) -> (String, String) {
    (id.to_owned(), title.to_owned())
}

#[tokio::test]
async fn offline_and_online_devices_and_the_server_converge() {
    let _serial = serial_guard().await;
    let Some(pg) = connect_or_skip().await else {
        return;
    };
    let pool = pg.fresh_database().await;
    let server = server(&pool, SyncPolicy::default()).await;
    let mut laptop = Device::new(&server, "alice").await;
    let mut phone = Device::new(&server, "alice").await;
    let mut bobs = Device::new(&server, "bob").await;

    // Laptop goes offline and works.
    laptop.write(
        "todo.create",
        json!({"id": "groceries", "title": "Buy milk"}),
    );
    laptop.write("todo.create", json!({"id": "taxes", "title": "File taxes"}));
    laptop.write(
        "todo.rename",
        json!({"id": "groceries", "title": "Buy oat milk"}),
    );
    assert_eq!(
        laptop.titles().len(),
        2,
        "offline writes are visible immediately"
    );

    // Phone is online the whole time.
    phone.write("todo.create", json!({"id": "call", "title": "Call mum"}));
    phone.sync(&server).await;
    assert_eq!(phone.titles(), vec![pair("call", "Call mum")]);

    // A server-side write that never went through sync (admin tool, REST).
    sqlx::query("UPDATE todos SET title = 'Call mum (Sunday)' WHERE id = 'call'")
        .execute(&pool)
        .await
        .unwrap();

    // Laptop reconnects.
    laptop.sync(&server).await;
    phone.sync(&server).await;
    bobs.sync(&server).await;

    let expected = vec![
        pair("call", "Call mum (Sunday)"),
        pair("groceries", "Buy oat milk"),
        pair("taxes", "File taxes"),
    ];
    assert_eq!(laptop.titles(), expected);
    assert_eq!(phone.titles(), expected);
    assert!(
        bobs.titles().is_empty(),
        "bob's scope sees none of alice's rows"
    );
    for device in [&laptop, &phone] {
        let status = device.engine.status(&device.conn).unwrap();
        assert_eq!(status.pending, 0, "every mutation acknowledged");
    }

    // Bob tries to rename alice's todo: rejected by the server's rule,
    // and his optimistic local effect (a no-op here) is rolled back.
    bobs.write("todo.rename", json!({"id": "call", "title": "hijacked"}));
    bobs.sync(&server).await;
    let status = bobs.engine.status(&bobs.conn).unwrap();
    assert_eq!(status.rejected.len(), 1);
    phone.sync(&server).await;
    assert_eq!(phone.titles()[0], pair("call", "Call mum (Sunday)"));
}
