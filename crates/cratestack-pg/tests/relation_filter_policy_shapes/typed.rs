//! Review follow-up, two gaps the first regression files did not pin: the
//! typed builder's multi-hop `RelPath` accessors (every hop past the root
//! carries its own scope), and which policy slot a relation subquery uses.

use super::{NONE, alice, ids, matched, pg, setup};
use crate::cratestack_schema::rv_shelf;

/// `rv_shelf::folder().doc()`: `doc()` is a `RelPath` accessor, not a root,
/// and must carry `RvDoc`'s scope.
#[tokio::test]
async fn typed_multi_hop_paths_scope_every_hop() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, db, _r)) = setup().await else {
        return;
    };
    let ctx = alice();
    let filtered = |filter| async {
        let rows = db
            .rv_shelf()
            .find_many()
            .where_expr(filter)
            .run(&ctx)
            .await
            .unwrap();
        let mut ids = rows.iter().map(|row| row.id).collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    };
    // Doc 21 is denied at the second hop, behind a readable folder (31).
    assert_eq!(
        filtered(rv_shelf::folder().doc().code().eq("D2".to_owned())).await,
        NONE
    );
    assert_eq!(
        filtered(rv_shelf::folder().doc().code().eq("D1".to_owned())).await,
        vec![40]
    );
    let sorted = |clause| async {
        let rows = db
            .rv_shelf()
            .find_many()
            .order_by(clause)
            .order_by(rv_shelf::id().asc())
            .run(&ctx)
            .await
            .unwrap();
        rows.iter().map(|row| row.id).collect::<Vec<_>>()
    };
    assert_eq!(
        sorted(rv_shelf::folder().doc().code().asc()).await,
        vec![44, 40, 41, 42, 43]
    );
    assert_eq!(
        sorted(rv_shelf::folder().doc().code().desc()).await,
        vec![40, 44, 41, 42, 43]
    );
}

/// A relation subquery applies the related model's *list* slot
/// (`list`/`read`) — the slot `?include=` reads through (`find_many`) and
/// the slot under which the caller could filter the related model
/// directly — not the `detail` slot. A row only a detail read may see is
/// absent from both.
#[tokio::test]
async fn relation_filters_use_the_list_slot_like_include() {
    let _guard = pg::serial_guard().await;
    let Some((_pg, _db, r)) = setup().await else {
        return;
    };
    assert_eq!(matched(&r, "/rv_slots").await, vec![80]);
    assert_eq!(
        matched(&r, "/rv_slot_links?slot.code=LISTED").await,
        vec![85]
    );
    assert_eq!(
        matched(&r, "/rv_slot_links?slot.code=DETAIL-ONLY").await,
        NONE
    );
    assert_eq!(
        ids(&r, "/rv_slot_links?sort=-slot.code,id").await,
        vec![85, 86]
    );
}
