//! The same scoping through the non-REST surfaces: the typed Rust builder
//! (filters and sorts), `@@paged` `total_count`, the bulk writes
//! (`update_many`/`delete_many` accept relation filters in-process), and
//! `preview_scoped_sql`.

use cratestack::sqlx::postgres::PgPoolOptions;

use super::{NONE, caller_one, get, matched, pg, setup};
use crate::cratestack_schema::{self, rf_link};

#[tokio::test]
async fn typed_builder_filters_and_sorts_apply_the_related_scope() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, db, _r)) = setup().await else {
        return;
    };
    let ctx = caller_one();
    let hidden = db
        .rf_link()
        .find_many()
        .where_expr(rf_link::hidden().code().eq("CODE-A".to_owned()))
        .run(&ctx)
        .await
        .unwrap();
    assert!(hidden.is_empty(), "typed filter on a hidden row matched");
    let tags = db
        .rf_link()
        .find_many()
        .where_expr(rf_link::tags().some().label().eq("t-a".to_owned()))
        .run(&ctx)
        .await
        .unwrap();
    assert!(
        tags.is_empty(),
        "typed to-many filter on hidden children matched"
    );
    let mine = db
        .rf_link()
        .find_many()
        .where_expr(rf_link::owned().code().eq("MINE".to_owned()))
        .run(&ctx)
        .await
        .unwrap();
    assert_eq!(mine.iter().map(|row| row.id).collect::<Vec<_>>(), vec![50]);

    let order = |clause| async {
        let rows = db
            .rf_link()
            .find_many()
            .order_by(clause)
            .order_by(rf_link::id().asc())
            .run(&ctx)
            .await
            .unwrap();
        rows.iter().map(|row| row.id).collect::<Vec<_>>()
    };
    assert_eq!(order(rf_link::hidden().score().desc()).await, vec![50, 51]);
    assert_eq!(order(rf_link::hidden().score().asc()).await, vec![50, 51]);
    assert_eq!(order(rf_link::public().code().desc()).await, vec![51, 50]);
}

#[tokio::test]
async fn paged_total_count_does_not_count_through_hidden_rows() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    let page = get(&r, "/rf_paged_links?hidden.code=CODE-A&limit=1").await;
    assert_eq!(page["items"], cratestack::serde_json::json!([]));
    assert_eq!(page["totalCount"], 0);
    // Positive control: rows 120 and 122 link to the caller's own row.
    let page = get(&r, "/rf_paged_links?owned.code=MINE&limit=1").await;
    assert_eq!(page["totalCount"], 2);
}

#[tokio::test]
async fn bulk_writes_scope_their_relation_filters_with_the_related_read_policy() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, db, r)) = setup().await else {
        return;
    };
    let ctx = caller_one();
    let set = || cratestack_schema::UpdateRfLinkInput {
        hiddenId: None,
        deniedId: None,
        ownedId: None,
        softId: None,
        internalId: None,
        publicId: Some(98),
    };
    let updated = db
        .rf_link()
        .update_many()
        .where_expr(rf_link::hidden().code().eq("CODE-A".to_owned()))
        .set(set())
        .run(&ctx)
        .await
        .unwrap();
    assert_eq!(updated.total, 0, "update_many matched through a hidden row");
    let deleted = db
        .rf_link()
        .delete_many()
        .where_expr(rf_link::soft().code().eq("S-GONE".to_owned()))
        .run(&ctx)
        .await
        .unwrap();
    assert_eq!(
        deleted.total, 0,
        "delete_many matched through a tombstoned row"
    );
    assert_eq!(matched(&r, "/rf_links?public.code=P-B").await, vec![51]);

    // Positive control: a readable related row still selects the write.
    let updated = db
        .rf_link()
        .update_many()
        .where_expr(rf_link::owned().code().eq("MINE".to_owned()))
        .set(set())
        .run(&ctx)
        .await
        .unwrap();
    assert_eq!(updated.total, 1);
    assert_eq!(matched(&r, "/rf_links?public.code=P-A").await, NONE);
}

/// `preview_scoped_sql` must show the related scope that executes. No
/// database: the pool is lazy and never connected.
#[tokio::test]
async fn scoped_preview_shows_the_related_scope() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused@localhost/unused")
        .unwrap();
    let db = cratestack_schema::Cratestack::builder(pool).build();
    let sql = db
        .rf_link()
        .find_many()
        .where_expr(rf_link::soft().code().eq("S-LIVE".to_owned()))
        .order_by(rf_link::owned().code().asc())
        .preview_scoped_sql(&caller_one());
    assert_eq!(
        sql,
        "SELECT id AS \"id\", hidden_id AS \"hiddenId\", denied_id AS \"deniedId\", \
         owned_id AS \"ownedId\", soft_id AS \"softId\", internal_id AS \"internalId\", \
         public_id AS \"publicId\" FROM rf_links WHERE EXISTS (SELECT 1 FROM rf_softs WHERE \
         rf_softs.id = rf_links.soft_id AND rf_softs.deleted_at IS NULL AND (TRUE) AND \
         code = $1) AND (TRUE) ORDER BY (SELECT rf_owneds.code FROM rf_owneds WHERE \
         rf_owneds.id = rf_links.owned_id AND (owner_id = $2) LIMIT 1) ASC NULLS LAST, \
         id ASC NULLS LAST",
    );
}
