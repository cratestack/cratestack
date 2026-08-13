//! Real end-to-end proof that `@@paged` compiles under
//! `include_embedded_schema!` (it used to be a hard `compile_error!` —
//! see `cratestack-macros/src/include/embedded.rs`'s module doc for why
//! that was wrong) and that the generated model's delegate gets real
//! pagination via `FindMany::paginate`, backed by an actual `COUNT(*)`.

use cratestack::PageInput;
use cratestack::include_embedded_schema;
use cratestack_rusqlite::{ModelDelegate, ddl::create_table_sql};

include_embedded_schema!("tests/fixtures/paged_model.cstack");

use cratestack_schema::CreatePostInput;
use cratestack_schema::POST_MODEL;
use cratestack_schema::models::Post;

fn setup() -> cratestack::RusqliteRuntime {
    let runtime = cratestack::RusqliteRuntime::open_in_memory().expect("open in-memory sqlite");
    runtime
        .with_connection(|conn| {
            conn.execute_batch(&create_table_sql(&POST_MODEL))
                .expect("apply DDL");
            Ok(())
        })
        .unwrap();
    runtime
}

#[test]
fn paged_model_delegate_paginates_with_a_real_count() {
    let runtime = setup();
    let delegate = ModelDelegate::<Post, i64>::new(&runtime, &POST_MODEL);
    for i in 0..5i64 {
        delegate
            .create(CreatePostInput {
                id: i + 1,
                title: format!("post-{i}"),
                published: i % 2 == 0,
            })
            .run()
            .expect("create succeeds");
    }

    let page = delegate
        .find_many()
        .paginate(PageInput {
            limit: Some(2),
            offset: Some(0),
        })
        .expect("paginate succeeds");

    assert_eq!(page.items.len(), 2);
    assert_eq!(page.total_count, Some(5));
    assert!(page.page_info.has_next_page);
    assert!(!page.page_info.has_previous_page);
}
