use cratestack::axum::body::Body;
use cratestack::axum::http::{Request, StatusCode};
use cratestack::include_server_schema;
use cratestack::sqlx::postgres::PgPoolOptions;
use cratestack::tracing::Subscriber;
use cratestack::{AuthProvider, CodecSet, CoolCodec, CoolContext, RequestContext, Value};
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_json::JsonCodec;
use tower::util::ServiceExt;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

include_server_schema!("tests/fixtures/blog.cstack", db = Postgres);

mod advanced_policy_schema {
    use super::*;

    include_server_schema!("tests/fixtures/advanced_policy.cstack", db = Postgres);

    fn advanced_test_db() -> cratestack_schema::Cratestack {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
            .expect("lazy pool should parse");
        cratestack_schema::Cratestack::builder(pool).build()
    }

    #[derive(Clone)]
    struct AdvancedProcedures {
        invocations: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    #[derive(Clone)]
    struct AdvancedPolicyRouteAuthProvider;

    impl AuthProvider for AdvancedPolicyRouteAuthProvider {
        type Error = cratestack::CoolError;

        fn authenticate(
            &self,
            request: &RequestContext<'_>,
        ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
            let mut fields = Vec::new();

            if let Some(id) = request
                .headers
                .get("x-auth-id")
                .and_then(|value| value.to_str().ok())
            {
                let id = match id.parse::<i64>() {
                    Ok(id) => id,
                    Err(error) => {
                        return core::future::ready(Err(cratestack::CoolError::BadRequest(
                            error.to_string(),
                        )));
                    }
                };
                fields.push(("id".to_owned(), Value::Int(id)));
            }

            if let Some(role) = request
                .headers
                .get("x-role")
                .and_then(|value| value.to_str().ok())
            {
                fields.push(("role".to_owned(), Value::String(role.to_owned())));
            }

            if let Some(email) = request
                .headers
                .get("x-email")
                .and_then(|value| value.to_str().ok())
            {
                fields.push(("email".to_owned(), Value::String(email.to_owned())));
            }

            core::future::ready(Ok(if fields.is_empty() {
                CoolContext::anonymous()
            } else {
                CoolContext::authenticated(fields)
            }))
        }
    }

    impl cratestack_schema::procedures::ProcedureRegistry for AdvancedProcedures {
        fn approve_post(
            &self,
            _db: &cratestack_schema::Cratestack,
            _ctx: &CoolContext,
            args: cratestack_schema::procedures::approve_post::Args,
        ) -> impl core::future::Future<
            Output = Result<
                cratestack_schema::procedures::approve_post::Output,
                cratestack::CoolError,
            >,
        > + Send {
            let invocations = std::sync::Arc::clone(&self.invocations);
            async move {
                invocations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(cratestack_schema::Post {
                    id: args.args.postId,
                    title: "Approved".to_owned(),
                    published: args.args.publishNow,
                    authorId: 1,
                })
            }
        }

        fn review_post(
            &self,
            _db: &cratestack_schema::Cratestack,
            _ctx: &CoolContext,
            args: cratestack_schema::procedures::review_post::Args,
        ) -> impl core::future::Future<
            Output = Result<
                cratestack_schema::procedures::review_post::Output,
                cratestack::CoolError,
            >,
        > + Send {
            async move {
                Ok(cratestack_schema::Post {
                    id: args.args.postId,
                    title: if args.args.dryRun {
                        "Dry Run"
                    } else {
                        "Reviewed"
                    }
                    .to_owned(),
                    published: args.args.publishNow,
                    authorId: 1,
                })
            }
        }
    }

    #[tokio::test]
    async fn bind_auth_exposes_scoped_delegate_run_api() {
        let db = advanced_test_db();

        let bound = db
            .bind_auth(Some(cratestack::serde_json::json!({
                "id": 7,
                "role": "admin",
                "email": "owner@example.com"
            })))
            .expect("principal should bind");

        let sql = bound.post().find_many().preview_scoped_sql();

        assert!(sql.contains("published = TRUE"));
        assert!(sql.contains("email = $1"));
    }

    #[tokio::test]
    async fn advanced_read_policy_renders_and_relation_auth_checks() {
        let db = advanced_test_db();
        let ctx = CoolContext::authenticated([
            ("id".to_owned(), Value::Int(42)),
            ("role".to_owned(), Value::String("admin".to_owned())),
            (
                "email".to_owned(),
                Value::String("owner@example.com".to_owned()),
            ),
        ]);

        let sql = db
            .post()
            .update(9)
            .set(cratestack_schema::UpdatePostInput::default())
            .preview_sql();
        assert!(sql.contains("UPDATE posts SET "));

        let scoped_sql = db.post().find_many().preview_scoped_sql(&ctx);
        assert!(scoped_sql.contains("published = TRUE"));
        assert!(scoped_sql.contains("email = $1"));
        assert!(scoped_sql.contains("banned = TRUE"));
    }

    #[tokio::test]
    async fn advanced_procedure_policy_supports_and_expressions() {
        let allowed = cratestack_schema::procedures::approve_post::authorize(
            &cratestack_schema::procedures::approve_post::Args {
                args: cratestack_schema::ApprovePostInput {
                    postId: 1,
                    publishNow: true,
                },
            },
            &CoolContext::authenticated([
                ("id".to_owned(), Value::Int(1)),
                ("role".to_owned(), Value::String("admin".to_owned())),
            ]),
        );
        assert!(allowed.is_ok());

        let denied = cratestack_schema::procedures::approve_post::authorize(
            &cratestack_schema::procedures::approve_post::Args {
                args: cratestack_schema::ApprovePostInput {
                    postId: 1,
                    publishNow: false,
                },
            },
            &CoolContext::authenticated([
                ("id".to_owned(), Value::Int(1)),
                ("role".to_owned(), Value::String("admin".to_owned())),
            ]),
        );
        assert!(denied.is_err());

        let deny_override = cratestack_schema::procedures::approve_post::authorize(
            &cratestack_schema::procedures::approve_post::Args {
                args: cratestack_schema::ApprovePostInput {
                    postId: 2,
                    publishNow: true,
                },
            },
            &CoolContext::authenticated([
                ("id".to_owned(), Value::Int(1)),
                ("role".to_owned(), Value::String("admin".to_owned())),
            ]),
        );
        assert!(matches!(
            deny_override,
            Err(cratestack::CoolError::Forbidden(_))
        ));

        let invoked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let invoked_flag = std::sync::Arc::clone(&invoked);
        let invoke_result = cratestack_schema::procedures::approve_post::invoke(
            &cratestack_schema::procedures::approve_post::Args {
                args: cratestack_schema::ApprovePostInput {
                    postId: 2,
                    publishNow: true,
                },
            },
            &CoolContext::authenticated([
                ("id".to_owned(), Value::Int(1)),
                ("role".to_owned(), Value::String("admin".to_owned())),
            ]),
            move || async move {
                invoked_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok::<_, cratestack::CoolError>(())
            },
        )
        .await;
        assert!(matches!(
            invoke_result,
            Err(cratestack::CoolError::Forbidden(_))
        ));
        assert!(!invoked.load(std::sync::atomic::Ordering::SeqCst));

        let anonymous_dry_run = cratestack_schema::procedures::review_post::authorize(
            &cratestack_schema::procedures::review_post::Args {
                args: cratestack_schema::ReviewPostInput {
                    postId: 3,
                    publishNow: false,
                    dryRun: true,
                    ownerEmail: "owner@example.com".to_owned(),
                    mirrorEmail: "owner@example.com".to_owned(),
                },
            },
            &CoolContext::anonymous(),
        );
        assert!(anonymous_dry_run.is_ok());

        let admin_review = cratestack_schema::procedures::review_post::authorize(
            &cratestack_schema::procedures::review_post::Args {
                args: cratestack_schema::ReviewPostInput {
                    postId: 3,
                    publishNow: true,
                    dryRun: false,
                    ownerEmail: "owner@example.com".to_owned(),
                    mirrorEmail: "owner@example.com".to_owned(),
                },
            },
            &CoolContext::authenticated([
                ("id".to_owned(), Value::Int(1)),
                ("role".to_owned(), Value::String("admin".to_owned())),
                (
                    "email".to_owned(),
                    Value::String("owner@example.com".to_owned()),
                ),
            ]),
        );
        assert!(admin_review.is_ok());

        let mismatched_input_fields = cratestack_schema::procedures::review_post::authorize(
            &cratestack_schema::procedures::review_post::Args {
                args: cratestack_schema::ReviewPostInput {
                    postId: 3,
                    publishNow: true,
                    dryRun: false,
                    ownerEmail: "owner@example.com".to_owned(),
                    mirrorEmail: "other@example.com".to_owned(),
                },
            },
            &CoolContext::authenticated([
                ("id".to_owned(), Value::Int(1)),
                ("role".to_owned(), Value::String("admin".to_owned())),
                (
                    "email".to_owned(),
                    Value::String("owner@example.com".to_owned()),
                ),
            ]),
        );
        assert!(matches!(
            mismatched_input_fields,
            Err(cratestack::CoolError::Forbidden(_))
        ));

        let mismatched_auth = cratestack_schema::procedures::review_post::authorize(
            &cratestack_schema::procedures::review_post::Args {
                args: cratestack_schema::ReviewPostInput {
                    postId: 3,
                    publishNow: true,
                    dryRun: false,
                    ownerEmail: "owner@example.com".to_owned(),
                    mirrorEmail: "owner@example.com".to_owned(),
                },
            },
            &CoolContext::authenticated([
                ("id".to_owned(), Value::Int(1)),
                ("role".to_owned(), Value::String("admin".to_owned())),
                (
                    "email".to_owned(),
                    Value::String("other@example.com".to_owned()),
                ),
            ]),
        );
        assert!(matches!(
            mismatched_auth,
            Err(cratestack::CoolError::Forbidden(_))
        ));
    }

    #[tokio::test]
    async fn advanced_procedure_deny_applies_at_route_level() {
        let codec = CborCodec;
        let invocations = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let router = cratestack_schema::axum::procedure_router(
            advanced_test_db(),
            AdvancedProcedures {
                invocations: std::sync::Arc::clone(&invocations),
            },
            codec.clone(),
            AdvancedPolicyRouteAuthProvider,
        );
        let body = codec
            .encode(&cratestack_schema::procedures::approve_post::Args {
                args: cratestack_schema::ApprovePostInput {
                    postId: 2,
                    publishNow: true,
                },
            })
            .expect("request body should encode");

        let denied = router
            .oneshot(
                Request::post("/$procs/approvePost")
                    .header("content-type", CborCodec::CONTENT_TYPE)
                    .header("accept", CborCodec::CONTENT_TYPE)
                    .header("x-auth-id", "1")
                    .header("x-role", "admin")
                    .body(Body::from(body))
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert_eq!(invocations.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}

mod enum_schema {
    use super::*;
    use cratestack::{CreateModelInput, ProcedureArgs, SqlValue};

    include_server_schema!("tests/fixtures/enums.cstack", db = Postgres);

    #[test]
    fn macro_generates_enum_summary_constants() {
        assert_eq!(cratestack_schema::ENUM_COUNT, 1);
        assert_eq!(cratestack_schema::ENUMS, &["Role"]);

        let summary = cratestack_schema::schema_summary();
        assert_eq!(summary.enums, cratestack_schema::ENUMS);
    }

    #[test]
    fn generated_enum_serializes_and_parses_by_schema_variant_name() {
        let json = cratestack::serde_json::to_value(cratestack_schema::Role::admin)
            .expect("enum should serialize");
        assert_eq!(
            json,
            cratestack::serde_json::Value::String("admin".to_owned())
        );

        let parsed: cratestack_schema::Role =
            cratestack::serde_json::from_value(json).expect("enum should deserialize");
        assert_eq!(parsed, cratestack_schema::Role::admin);
        assert_eq!(parsed.to_string(), "admin");
    }

    #[test]
    fn generated_procedure_args_expose_enum_values_to_policy_runtime() {
        let args = cratestack_schema::procedures::resolve_user::Args {
            role: cratestack_schema::Role::admin,
            args: cratestack_schema::RoleFilter {
                role: cratestack_schema::Role::member,
            },
        };

        assert_eq!(
            args.procedure_arg_value("role"),
            Some(Value::String("admin".to_owned()))
        );
        assert_eq!(
            args.procedure_arg_value("args.role"),
            Some(Value::String("member".to_owned()))
        );
    }

    #[test]
    fn generated_model_inputs_encode_enum_fields_as_sql_strings() {
        let input = cratestack_schema::CreateUserInput {
            role: cratestack_schema::Role::admin,
        };
        let values = input.sql_values();

        assert_eq!(values.len(), 1);
        assert_eq!(values[0].column, "role");
        assert_eq!(values[0].value, SqlValue::String("admin".to_owned()));
    }
}

mod auth_engine_schema {
    use super::*;

    include_server_schema!("tests/fixtures/auth_engine.cstack", db = Postgres);

    fn tenant_scope(id: &str) -> cratestack::Value {
        cratestack::Value::Map(std::collections::BTreeMap::from([(
            "id".to_owned(),
            cratestack::Value::String(id.to_owned()),
        )]))
    }

    fn auth_test_db() -> cratestack_schema::Cratestack {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
            .expect("lazy pool should parse");
        cratestack_schema::Cratestack::builder(pool).build()
    }

    #[test]
    fn create_todo_input_uses_auth_default_and_omits_organization_id() {
        let _input = cratestack_schema::CreateTodoInput {
            ownerId: "usr_1".to_owned(),
            title: "Plan rollout".to_owned(),
        };

        let _note = cratestack_schema::CreateScopedNoteInput {
            body: "Scoped body".to_owned(),
        };
    }

    #[tokio::test]
    async fn preview_sql_supports_all_and_deny_rules() {
        let db = auth_test_db();

        let scoped = db
            .bind_auth(Some(cratestack::serde_json::json!({
                "id": "usr_1",
                "userId": "usr_1",
                "role": "admin",
                "organization": { "id": "org_1" },
                "tenant": { "id": "tenant_1" },
                "organizationRole": "member"
            })))
            .expect("principal should bind");

        let post_sql = scoped.post().find_many().preview_scoped_sql();
        assert!(post_sql.contains("author_id = "));
        assert!(post_sql.contains("published = TRUE"));

        let todo_sql = scoped.todo().find_many().preview_scoped_sql();
        assert!(todo_sql.contains("organization_id != "));
        assert!(todo_sql.contains("owner_id = "));

        let admin_panel_sql = scoped.admin_panel().find_many().preview_scoped_sql();
        assert!(admin_panel_sql.contains("TRUE"));
    }

    #[test]
    fn built_in_policy_functions_authorize_procedures() {
        let allowed = cratestack_schema::procedures::admin_pulse::authorize(
            &cratestack_schema::procedures::admin_pulse::Args {
                args: cratestack_schema::InspectPostInput {
                    postId: "post_1".to_owned(),
                },
            },
            &CoolContext::authenticated([
                ("role".to_owned(), Value::String("admin".to_owned())),
                ("tenant".to_owned(), tenant_scope("tenant_1")),
            ]),
        );
        assert!(allowed.is_ok());

        let denied_role = cratestack_schema::procedures::admin_pulse::authorize(
            &cratestack_schema::procedures::admin_pulse::Args {
                args: cratestack_schema::InspectPostInput {
                    postId: "post_1".to_owned(),
                },
            },
            &CoolContext::authenticated([
                ("role".to_owned(), Value::String("member".to_owned())),
                ("tenant".to_owned(), tenant_scope("tenant_1")),
            ]),
        );
        assert!(matches!(
            denied_role,
            Err(cratestack::CoolError::Forbidden(_))
        ));

        let denied_tenant = cratestack_schema::procedures::admin_pulse::authorize(
            &cratestack_schema::procedures::admin_pulse::Args {
                args: cratestack_schema::InspectPostInput {
                    postId: "post_1".to_owned(),
                },
            },
            &CoolContext::authenticated([
                ("role".to_owned(), Value::String("admin".to_owned())),
                ("tenant".to_owned(), tenant_scope("tenant_2")),
            ]),
        );
        assert!(matches!(
            denied_tenant,
            Err(cratestack::CoolError::Forbidden(_))
        ));
    }
}

#[derive(Clone)]
struct TestProcedures;

impl cratestack_schema::procedures::ProcedureRegistry for TestProcedures {
    fn get_feed(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::get_feed::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::get_feed::Output, cratestack::CoolError>,
    > + Send {
        async move {
            Ok(vec![cratestack_schema::Post {
                id: args.limit.unwrap_or(1),
                title: "Feed".to_owned(),
                subtitle: None,
                published: true,
                authorId: 1,
            }])
        }
    }

    fn get_feed_page(
        &self,
        _db: &cratestack_schema::Cratestack,
        _ctx: &CoolContext,
        args: cratestack_schema::procedures::get_feed_page::Args,
    ) -> impl core::future::Future<
        Output = Result<
            cratestack_schema::procedures::get_feed_page::Output,
            cratestack::CoolError,
        >,
    > + Send {
        async move {
            let limit = args.limit.unwrap_or(1);
            let offset = args.offset.unwrap_or(0);
            Ok(cratestack::Page::new(
                vec![cratestack_schema::Post {
                    id: limit + offset,
                    title: "Feed Page".to_owned(),
                    subtitle: Some("paged".to_owned()),
                    published: true,
                    authorId: 1,
                }],
                cratestack::PageInfo {
                    limit: Some(limit),
                    offset: Some(offset),
                    has_next_page: true,
                    has_previous_page: offset > 0,
                },
            )
            .with_total_count(Some(3)))
        }
    }

    fn publish_post(
        &self,
        _db: &cratestack_schema::Cratestack,
        ctx: &CoolContext,
        args: cratestack_schema::procedures::publish_post::Args,
    ) -> impl core::future::Future<
        Output = Result<cratestack_schema::procedures::publish_post::Output, cratestack::CoolError>,
    > + Send {
        let author_id = match ctx.auth_field("id") {
            Some(Value::Int(id)) => *id,
            _ => 0,
        };
        async move {
            Ok(cratestack_schema::Post {
                id: args.args.postId,
                title: "Published".to_owned(),
                subtitle: None,
                published: true,
                authorId: author_id,
            })
        }
    }
}

fn test_db() -> cratestack_schema::Cratestack {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    cratestack_schema::Cratestack::builder(pool).build()
}

fn test_model_router(codec: CborCodec) -> cratestack::axum::Router {
    cratestack_schema::axum::model_router(test_db(), codec, TestAuthProvider)
}

fn test_procedure_router(codec: CborCodec) -> cratestack::axum::Router {
    cratestack_schema::axum::procedure_router(test_db(), TestProcedures, codec, TestAuthProvider)
}

fn test_combined_router(codec: CborCodec) -> cratestack::axum::Router {
    cratestack_schema::axum::router(test_db(), TestProcedures, codec, TestAuthProvider)
}

fn test_negotiated_procedure_router() -> cratestack::axum::Router {
    cratestack_schema::axum::procedure_router(
        test_db(),
        TestProcedures,
        CodecSet::new(CborCodec, JsonCodec),
        TestAuthProvider,
    )
}

#[test]
fn generated_axum_route_transport_metadata_is_public() {
    let feed = cratestack_schema::axum::PROCEDURE_GET_FEED_POST;
    assert_eq!(feed.method, "POST");
    assert_eq!(feed.path, "/$procs/getFeed");
    assert!(feed.capabilities.supports_sequence_response);
    assert!(
        feed.capabilities
            .response_types
            .contains(&cratestack::CBOR_SEQUENCE_CONTENT_TYPE)
    );

    let publish = cratestack_schema::axum::PROCEDURE_PUBLISH_POST_POST;
    assert_eq!(publish.method, "POST");
    assert_eq!(publish.path, "/$procs/publishPost");
    assert!(!publish.capabilities.supports_sequence_response);
    assert!(
        !publish
            .capabilities
            .response_types
            .contains(&cratestack::CBOR_SEQUENCE_CONTENT_TYPE)
    );

    let feed_page = cratestack_schema::axum::PROCEDURE_GET_FEED_PAGE_POST;
    assert_eq!(feed_page.method, "POST");
    assert_eq!(feed_page.path, "/$procs/getFeedPage");
    assert!(!feed_page.capabilities.supports_sequence_response);
}

#[test]
fn generated_axum_route_transport_registry_lists_routes() {
    let routes = cratestack_schema::axum::ROUTE_TRANSPORTS;
    assert!(routes.iter().any(|route| route.path == "/$procs/getFeed"
        && route.method == "POST"
        && route.capabilities.supports_sequence_response));
    assert!(
        routes
            .iter()
            .any(|route| route.path == "/posts" && route.method == "GET")
    );
    assert!(
        routes
            .iter()
            .any(|route| route.path == "/posts/{id}" && route.method == "PATCH")
    );
}

#[tokio::test]
async fn generated_event_subscription_api_is_public() {
    let db = test_db();
    db.events().on_session_created(move |event| async move {
        let _ = event.data.id;
        Ok(())
    });
    db.events().on_post_deleted(|event| async move {
        let _ = event.data.id;
        Ok(())
    });
}

fn decode_cbor_seq<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Vec<T> {
    let mut values = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let mut deserializer = minicbor_serde::Deserializer::new(&bytes[offset..]);
        values.push(T::deserialize(&mut deserializer).expect("cbor-seq item should decode"));
        let consumed = deserializer.decoder().position();
        assert!(consumed > 0, "cbor-seq decoder should make progress");
        offset += consumed;
    }
    values
}

#[derive(Clone, Default)]
struct EventCaptureLayer {
    events: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl EventCaptureLayer {
    fn snapshot(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("event capture mutex should not be poisoned")
            .clone()
    }
}

impl<S> Layer<S> for EventCaptureLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &cratestack::tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = TraceFieldVisitor::default();
        event.record(&mut visitor);
        self.events
            .lock()
            .expect("event capture mutex should not be poisoned")
            .push(format!(
                "{} {}",
                event.metadata().name(),
                visitor.fields.join(" ")
            ));
    }
}

#[derive(Default)]
struct TraceFieldVisitor {
    fields: Vec<String>,
}

impl cratestack::tracing::field::Visit for TraceFieldVisitor {
    fn record_debug(
        &mut self,
        field: &cratestack::tracing::field::Field,
        value: &dyn std::fmt::Debug,
    ) {
        self.fields.push(format!("{}={value:?}", field.name()));
    }

    fn record_i64(&mut self, field: &cratestack::tracing::field::Field, value: i64) {
        self.fields.push(format!("{}={value}", field.name()));
    }

    fn record_u64(&mut self, field: &cratestack::tracing::field::Field, value: u64) {
        self.fields.push(format!("{}={value}", field.name()));
    }

    fn record_bool(&mut self, field: &cratestack::tracing::field::Field, value: bool) {
        self.fields.push(format!("{}={value}", field.name()));
    }

    fn record_str(&mut self, field: &cratestack::tracing::field::Field, value: &str) {
        self.fields.push(format!("{}={value}", field.name()));
    }
}

fn resolve_test_context(
    headers: &cratestack::axum::http::HeaderMap,
) -> Result<CoolContext, cratestack::CoolError> {
    let mut fields = Vec::new();
    if let Some(role) = headers.get("x-role") {
        let role = role
            .to_str()
            .map_err(|error| cratestack::CoolError::BadRequest(error.to_string()))?;
        fields.push(("role".to_owned(), Value::String(role.to_owned())));
    }
    if let Some(id) = headers.get("x-auth-id") {
        let id = id
            .to_str()
            .map_err(|error| cratestack::CoolError::BadRequest(error.to_string()))?
            .parse::<i64>()
            .map_err(|error| cratestack::CoolError::BadRequest(error.to_string()))?;
        fields.push(("id".to_owned(), Value::Int(id)));
    }

    if fields.is_empty() {
        Ok(CoolContext::anonymous())
    } else {
        Ok(CoolContext::authenticated(fields))
    }
}

#[derive(Clone)]
struct TestAuthProvider;

impl AuthProvider for TestAuthProvider {
    type Error = cratestack::CoolError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
        core::future::ready(resolve_test_context(request.headers))
    }
}

#[test]
fn macro_generates_schema_summary_constants() {
    assert_eq!(cratestack_schema::MODEL_COUNT, 4);
    assert_eq!(cratestack_schema::TYPE_COUNT, 1);
    assert_eq!(cratestack_schema::ENUM_COUNT, 0);
    assert_eq!(cratestack_schema::PROCEDURE_COUNT, 3);
    assert_eq!(
        cratestack_schema::MODELS,
        &["User", "Profile", "Post", "Session"]
    );
    assert_eq!(
        cratestack_schema::PROCEDURES,
        &["getFeed", "getFeedPage", "publishPost"]
    );
}

#[test]
fn generated_summary_matches_constants() {
    let summary = cratestack_schema::schema_summary();
    assert_eq!(summary.models, cratestack_schema::MODELS);
    assert_eq!(summary.types, cratestack_schema::TYPES);
    assert_eq!(summary.enums, cratestack_schema::ENUMS);
    assert_eq!(summary.procedures, cratestack_schema::PROCEDURES);
}

#[test]
fn generated_model_descriptor_exposes_query_contract_metadata() {
    let descriptor = &cratestack_schema::models::POST_MODEL;

    assert_eq!(
        descriptor.allowed_fields,
        &["id", "title", "subtitle", "published", "authorId"]
    );
    assert_eq!(descriptor.allowed_includes, &["author"]);
    assert!(descriptor.allowed_sorts.contains(&"id"));
    assert!(descriptor.allowed_sorts.contains(&"author.email"));
    assert!(
        descriptor
            .allowed_sorts
            .contains(&"author.profile.nickname")
    );
}

#[test]
fn generated_selection_builders_serialize_projection_contract() {
    let selection = cratestack_schema::post::select()
        .id()
        .title()
        .include_author_selected(cratestack_schema::user::include_selection().email());

    let query = selection.to_query();

    assert_eq!(query.fields, vec!["id".to_owned(), "title".to_owned()]);
    assert_eq!(query.includes, vec!["author".to_owned()]);
    assert_eq!(
        query.include_fields.get("author"),
        Some(&vec!["email".to_owned()])
    );
}

#[test]
fn generated_selection_decoders_project_root_and_included_to_one_fields() {
    let selection = cratestack_schema::post::select()
        .id()
        .title()
        .include_author_selected(cratestack_schema::user::include_selection().email());

    let selected = selection
        .decode_one(cratestack::serde_json::json!({
            "id": 1,
            "title": "Published Post",
            "author": {
                "email": "owner@example.com"
            }
        }))
        .expect("selected post should decode");

    assert_eq!(selected.id().expect("id should decode"), 1);
    assert_eq!(
        selected.title().expect("title should decode"),
        "Published Post"
    );
    assert!(selected.subtitle().is_err());
    let author = selected
        .author()
        .expect("author should decode")
        .expect("author should be present");
    assert_eq!(
        author.email().expect("email should decode"),
        "owner@example.com"
    );
}

#[test]
fn generated_selection_decoders_project_included_to_many_fields() {
    let selection = cratestack_schema::user::select()
        .id()
        .include_sessions_selected(cratestack_schema::session::include_selection().id().label());

    let selected = selection
        .decode_one(cratestack::serde_json::json!({
            "id": 1,
            "sessions": [
                { "id": "cprimarysession1", "label": "Primary Session" },
                { "id": "crevokedsession2", "label": "Revoked Session" }
            ]
        }))
        .expect("selected user should decode");

    let sessions = selected.sessions().expect("sessions should decode");
    assert_eq!(sessions.len(), 2);
    assert_eq!(
        sessions[0].id().expect("session id should decode"),
        "cprimarysession1"
    );
    assert_eq!(
        sessions[0].label().expect("session label should decode"),
        "Primary Session"
    );
}

#[test]
fn generated_selection_builders_support_nested_include_paths() {
    let selection = cratestack_schema::post::select()
        .id()
        .include_author_selected(
            cratestack_schema::user::include_selection()
                .email()
                .include_profile_selected(
                    cratestack_schema::profile::include_selection().nickname(),
                ),
        );

    let query = selection.to_query();

    assert_eq!(
        query.includes,
        vec!["author".to_owned(), "author.profile".to_owned()]
    );
    assert_eq!(
        query.include_fields.get("author"),
        Some(&vec!["email".to_owned()])
    );
    assert_eq!(
        query.include_fields.get("author.profile"),
        Some(&vec!["nickname".to_owned()])
    );
}

#[test]
fn generated_selection_decoders_project_nested_includes() {
    let selection = cratestack_schema::post::select()
        .id()
        .include_author_selected(
            cratestack_schema::user::include_selection()
                .email()
                .include_profile_selected(
                    cratestack_schema::profile::include_selection().nickname(),
                ),
        );

    let selected = selection
        .decode_one(cratestack::serde_json::json!({
            "id": 1,
            "author": {
                "email": "owner@example.com",
                "profile": {
                    "nickname": "Zulu"
                }
            }
        }))
        .expect("selected nested post should decode");

    let author = selected
        .author()
        .expect("author should decode")
        .expect("author should be present");
    assert_eq!(
        author.email().expect("email should decode"),
        "owner@example.com"
    );
    let profile = author
        .profile()
        .expect("profile should decode")
        .expect("profile should be present");
    assert_eq!(profile.nickname().expect("nickname should decode"), "Zulu");
}

#[tokio::test]
async fn generated_delegate_previews_snake_case_select_sql() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool.post().find_many().limit(20).offset(5).preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\" FROM posts LIMIT $1 OFFSET $2"
    );
}

#[tokio::test]
async fn generated_where_and_order_preview_select_sql() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .post()
        .find_many()
        .where_(cratestack_schema::post::published().is_true())
        .where_(cratestack_schema::post::authorId().ne(42_i64))
        .where_(cratestack_schema::post::title().contains("Hel"))
        .where_(cratestack_schema::post::subtitle().is_null())
        .where_(cratestack_schema::post::id().in_([1_i64, 2_i64, 3_i64]))
        .order_by(cratestack_schema::post::title().asc())
        .order_by(cratestack_schema::post::id().desc())
        .limit(10)
        .offset(20)
        .preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\" FROM posts WHERE published = $1 AND author_id != $2 AND title LIKE $3 AND subtitle IS NULL AND id IN ($4, $5, $6) ORDER BY title ASC, id DESC LIMIT $7 OFFSET $8"
    );
}

#[tokio::test]
async fn generated_relation_order_preview_appends_primary_key_tie_break() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .post()
        .find_many()
        .order_by(cratestack_schema::post::author().email().desc())
        .preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\" FROM posts ORDER BY (SELECT users.email FROM users WHERE users.id = posts.author_id LIMIT 1) DESC NULLS LAST, id DESC"
    );
}

#[tokio::test]
async fn generated_nested_relation_order_preview_renders_nested_subqueries() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .post()
        .find_many()
        .order_by(cratestack_schema::post::author().profile().nickname().asc())
        .preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\" FROM posts ORDER BY (SELECT (SELECT profiles.nickname FROM profiles WHERE profiles.id = users.profile_id LIMIT 1) FROM users WHERE users.id = posts.author_id LIMIT 1) ASC NULLS LAST, id ASC"
    );
}

#[tokio::test]
async fn generated_typed_relation_filter_preview_renders_nested_exists() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .post()
        .find_many()
        .where_expr(
            cratestack_schema::post::author()
                .profile()
                .nickname()
                .eq("Zulu"),
        )
        .preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\" FROM posts WHERE EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND EXISTS (SELECT 1 FROM profiles WHERE profiles.id = users.profile_id AND nickname = $1))"
    );
}

#[tokio::test]
async fn generated_typed_to_many_filter_preview_renders_quantified_exists() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .user()
        .find_many()
        .where_expr(
            cratestack_schema::user::sessions()
                .some()
                .label()
                .contains("Revoked"),
        )
        .preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", email AS \"email\", role AS \"role\", profile_id AS \"profileId\" FROM users WHERE EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND label LIKE $1)"
    );
}

#[tokio::test]
async fn generated_typed_to_many_every_filter_preview_renders_quantified_not_exists() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .user()
        .find_many()
        .where_expr(
            cratestack_schema::user::sessions()
                .every()
                .revokedAt()
                .is_null(),
        )
        .preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", email AS \"email\", role AS \"role\", profile_id AS \"profileId\" FROM users WHERE NOT EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND NOT (revoked_at IS NULL))"
    );
}

#[tokio::test]
async fn generated_typed_to_many_none_filter_preview_renders_quantified_not_exists() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .user()
        .find_many()
        .where_expr(
            cratestack_schema::user::sessions()
                .none()
                .revokedAt()
                .is_null(),
        )
        .preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", email AS \"email\", role AS \"role\", profile_id AS \"profileId\" FROM users WHERE NOT EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND revoked_at IS NULL)"
    );
}

#[tokio::test]
async fn generated_builder_filter_composition_preview_renders_and_or_groups() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .post()
        .find_many()
        .where_expr(
            cratestack_schema::post::author()
                .profile()
                .nickname()
                .eq("Zulu")
                .and(cratestack_schema::post::published().is_true())
                .or(cratestack_schema::post::title().contains("Draft")),
        )
        .preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\" FROM posts WHERE ((EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND EXISTS (SELECT 1 FROM profiles WHERE profiles.id = users.profile_id AND nickname = $1)) AND published = $2) OR title LIKE $3)"
    );
}

#[tokio::test]
async fn generated_builder_filter_negation_preview_renders_not_group() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .post()
        .find_many()
        .where_expr(
            cratestack_schema::post::author()
                .profile()
                .nickname()
                .eq("Zulu")
                .and(cratestack_schema::post::published().is_true())
                .not(),
        )
        .preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\" FROM posts WHERE NOT ((EXISTS (SELECT 1 FROM users WHERE users.id = posts.author_id AND EXISTS (SELECT 1 FROM profiles WHERE profiles.id = users.profile_id AND nickname = $1)) AND published = $2))"
    );
}

#[tokio::test]
async fn generated_quantified_filter_composition_preview_renders_and_or_not_groups() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .user()
        .find_many()
        .where_expr(
            cratestack_schema::user::sessions()
                .some()
                .label()
                .contains("Primary")
                .and(
                    cratestack_schema::user::sessions()
                        .every()
                        .revokedAt()
                        .is_null(),
                )
                .or(cratestack_schema::user::email().contains("other"))
                .not(),
        )
        .preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", email AS \"email\", role AS \"role\", profile_id AS \"profileId\" FROM users WHERE NOT (((EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND label LIKE $1) AND NOT EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id AND NOT (revoked_at IS NULL))) OR email LIKE $2))"
    );
}

#[tokio::test]
async fn read_policies_scope_find_many_for_anonymous_context() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();
    let ctx = CoolContext::anonymous();

    let sql = cool
        .post()
        .find_many()
        .where_(cratestack_schema::post::title().contains("Hel"))
        .preview_scoped_sql(&ctx);

    // blog.cstack's Post `@@allow("list", ...)` now matches `published
    // || authorId == auth().id` (the policy was widened so owners can
    // see their own drafts via list — see the related fix in
    // policy_db.rs). Anonymous context has no `auth().id`, so the
    // second disjunct collapses to `FALSE`.
    assert_eq!(
        sql,
        "SELECT id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\" FROM posts WHERE title LIKE $1 AND ((published = TRUE OR FALSE))"
    );
}

#[tokio::test]
async fn read_policies_scope_find_many_for_authenticated_context() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();
    let ctx = CoolContext::authenticated([("id".to_owned(), Value::Int(42))]);

    let sql = cool.post().find_many().preview_scoped_sql(&ctx);

    // Authenticated context binds `auth().id` into the second disjunct
    // of the widened `@@allow("list", ...)` policy.
    assert_eq!(
        sql,
        "SELECT id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\" FROM posts WHERE (published = TRUE OR author_id = $1)"
    );
}

#[tokio::test]
async fn read_policies_default_deny_without_matching_context() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();
    let ctx = CoolContext::anonymous();

    let sql = cool.user().find_many().preview_scoped_sql(&ctx);

    assert_eq!(
        sql,
        "SELECT id AS \"id\", email AS \"email\", role AS \"role\", profile_id AS \"profileId\" FROM users WHERE FALSE"
    );
}

#[tokio::test]
async fn read_policies_scope_find_unique() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();
    let ctx = CoolContext::authenticated([("id".to_owned(), Value::Int(9))]);

    let sql = cool.post().find_unique(7_i64).preview_scoped_sql(&ctx);

    assert_eq!(
        sql,
        "SELECT id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\" FROM posts WHERE ((published = TRUE OR author_id = $1)) AND id = $2 LIMIT 1"
    );
}

#[tokio::test]
async fn generated_find_unique_targets_primary_key_column() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool.post().find_unique(7_i64).preview_sql();

    assert_eq!(
        sql,
        "SELECT id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\" FROM posts WHERE id = $1 LIMIT 1"
    );
}

#[tokio::test]
async fn generated_create_input_previews_insert_sql() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .post()
        .create(cratestack_schema::CreatePostInput {
            id: 7,
            title: "Hello".to_owned(),
            subtitle: None,
            published: true,
            authorId: 42,
        })
        .preview_sql();

    assert_eq!(
        sql,
        "INSERT INTO posts (id, title, subtitle, published, author_id) VALUES ($1, $2, $3, $4, $5) RETURNING id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\""
    );
}

#[tokio::test]
async fn generated_update_input_previews_partial_update_sql() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool
        .post()
        .update(7_i64)
        .set(cratestack_schema::UpdatePostInput {
            title: Some("Updated".to_owned()),
            subtitle: Some(None),
            published: None,
            authorId: Some(9),
        })
        .preview_sql();

    assert_eq!(
        sql,
        "UPDATE posts SET title = $1, subtitle = $2, author_id = $3 WHERE id = $4 RETURNING id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\""
    );
}

#[tokio::test]
async fn generated_delete_previews_delete_sql() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
        .expect("lazy pool should parse");
    let cool = cratestack_schema::Cratestack::builder(pool).build();

    let sql = cool.post().delete(11_i64).preview_sql();

    assert_eq!(
        sql,
        "DELETE FROM posts WHERE id = $1 RETURNING id AS \"id\", title AS \"title\", subtitle AS \"subtitle\", published AS \"published\", author_id AS \"authorId\""
    );
}

#[test]
fn generated_type_structs_are_available() {
    let input = cratestack_schema::PublishPostInput { postId: 5 };

    assert_eq!(input.postId, 5);
}

#[test]
fn generated_field_modules_are_available() {
    let _ = cratestack_schema::post::published().is_false();
    let _ = cratestack_schema::post::title().desc();
    let _ = cratestack_schema::post::subtitle().is_not_null();
    let _ = cratestack_schema::post::subtitle().starts_with("sub");
    let _ = cratestack_schema::post::author()
        .email()
        .eq("owner@example.com");
    let _ = cratestack_schema::post::author().email().desc();
    let _ = cratestack_schema::post::author()
        .profile()
        .nickname()
        .eq("Zulu");
    let _ = cratestack_schema::post::author().profile().nickname().asc();
    let _ = cratestack_schema::user::sessions()
        .some()
        .label()
        .contains("Revoked");
    let _ = cratestack_schema::user::sessions()
        .every()
        .revokedAt()
        .is_null();
    let _ = cratestack_schema::user::sessions()
        .none()
        .label()
        .starts_with("Blocked");
    let _ = cratestack_schema::post::author()
        .email()
        .eq("owner@example.com")
        .and(cratestack_schema::post::published().is_true())
        .or(cratestack_schema::post::title().contains("Post"));
    let _ = cratestack_schema::post::author()
        .profile()
        .nickname()
        .eq("Zulu")
        .not();
    let _ = cratestack_schema::post::author::email_eq("owner@example.com");
    let _ = cratestack_schema::post::author::email_desc();
    let _ = cratestack_schema::post::author::profile::nickname_eq("Zulu");
    let _ = cratestack_schema::post::author::profile::nickname_asc();
    let _ = cratestack_schema::user::sessions::some::label_contains("Revoked");
    let _ = cratestack_schema::user::sessions::every::revokedAt_is_null();
    let _ = cratestack_schema::user::sessions::none::label_starts_with("Blocked");
    let _ = cratestack_schema::session::createdAt().asc();
    let _ = cratestack_schema::session::externalId().desc();
    let _ = cratestack_schema::session::revokedAt().is_null();
}

#[tokio::test]
async fn procedure_policy_allows_admin_invocation() {
    let ctx = CoolContext::authenticated([("role".to_owned(), Value::String("admin".to_owned()))]);
    let input = cratestack_schema::PublishPostInput { postId: 8 };

    let value = cratestack_schema::procedures::publish_post::invoke(&input, &ctx, || async {
        Ok::<_, cratestack::CoolError>(input.postId)
    })
    .await
    .expect("admin invocation should be allowed");

    assert_eq!(value, 8);
}

#[tokio::test]
async fn procedure_policy_denies_non_admin_invocation() {
    let ctx = CoolContext::authenticated([("role".to_owned(), Value::String("member".to_owned()))]);
    let input = cratestack_schema::PublishPostInput { postId: 8 };

    let error = cratestack_schema::procedures::publish_post::invoke(&input, &ctx, || async {
        Ok::<_, cratestack::CoolError>(input.postId)
    })
    .await
    .expect_err("non-admin invocation should be denied");

    assert!(matches!(error, cratestack::CoolError::Forbidden(_)));
}

#[tokio::test]
async fn procedure_policy_allows_authenticated_feed_invocation() {
    let ctx = CoolContext::authenticated([("id".to_owned(), Value::Int(1))]);

    cratestack_schema::procedures::get_feed::authorize(&(), &ctx)
        .expect("authenticated feed access should be allowed");
}

#[tokio::test]
async fn procedure_policy_denies_anonymous_feed_invocation() {
    let ctx = CoolContext::anonymous();

    let error = cratestack_schema::procedures::get_feed::authorize(&(), &ctx)
        .expect_err("anonymous feed access should be denied");

    assert!(matches!(error, cratestack::CoolError::Forbidden(_)));
}

#[tokio::test]
async fn axum_procedure_route_allows_admin_invocation() {
    let codec = CborCodec;
    let router = test_procedure_router(codec.clone());
    let body = codec
        .encode(&cratestack_schema::procedures::publish_post::Args {
            args: cratestack_schema::PublishPostInput { postId: 44 },
        })
        .expect("request body should encode");

    let response = router
        .oneshot(
            Request::post("/$procs/publishPost")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-role", "admin")
                .header("x-auth-id", "9")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn negotiated_procedure_route_accepts_json_request_and_response() {
    let router = test_negotiated_procedure_router();
    let body = JsonCodec
        .encode(&cratestack_schema::procedures::publish_post::Args {
            args: cratestack_schema::PublishPostInput { postId: 44 },
        })
        .expect("request body should encode");

    let response = router
        .oneshot(
            Request::post("/$procs/publishPost")
                .header("content-type", JsonCodec::CONTENT_TYPE)
                .header("accept", JsonCodec::CONTENT_TYPE)
                .header("x-role", "admin")
                .header("x-auth-id", "9")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some(JsonCodec::CONTENT_TYPE)
    );
}

#[tokio::test]
async fn cbor_procedure_route_can_return_cbor_sequence_for_list_output() {
    let codec = CborCodec;
    let router = test_procedure_router(codec.clone());
    let body = codec
        .encode(&cratestack_schema::procedures::get_feed::Args { limit: Some(2) })
        .expect("request body should encode");

    let response = router
        .oneshot(
            Request::post("/$procs/getFeed")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("accept", cratestack::CBOR_SEQUENCE_CONTENT_TYPE)
                .header("x-auth-id", "9")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some(cratestack::CBOR_SEQUENCE_CONTENT_TYPE)
    );

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let values: Vec<cratestack_schema::Post> = decode_cbor_seq(bytes.as_ref());
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].title, "Feed");
}

#[tokio::test]
async fn procedure_route_can_return_paged_output() {
    let codec = CborCodec;
    let router = test_procedure_router(codec.clone());
    let body = codec
        .encode(&cratestack_schema::procedures::get_feed_page::Args {
            limit: Some(2),
            offset: Some(1),
        })
        .expect("request body should encode");

    let response = router
        .oneshot(
            Request::post("/$procs/getFeedPage")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "9")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some(CborCodec::CONTENT_TYPE)
    );

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should read");
    let page: cratestack::Page<cratestack_schema::Post> =
        codec.decode(&bytes).expect("paged response should decode");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].title, "Feed Page");
    assert_eq!(page.total_count, Some(3));
    assert_eq!(page.page_info.limit, Some(2));
    assert_eq!(page.page_info.offset, Some(1));
}

#[tokio::test(flavor = "current_thread")]
async fn generated_routes_emit_tracing_events() {
    // Scope the subscriber to the request future via `WithSubscriber`
    // instead of `set_default`. `set_default` installs a thread-local
    // default and returns a `!Send` guard; holding it across the await
    // happens to work on a current-thread runtime, but tests under high
    // parallel load have surfaced as flaky because the polling thread
    // can run other tasks between yields. `WithSubscriber` attaches the
    // dispatch to the future itself — events emitted while polling
    // *this* future see *this* subscriber, regardless of which thread
    // or runtime polls it.
    use cratestack::tracing::instrument::WithSubscriber;

    let codec = CborCodec;
    let router = test_procedure_router(codec.clone());
    let body = codec
        .encode(&cratestack_schema::procedures::get_feed_page::Args {
            limit: Some(2),
            offset: Some(1),
        })
        .expect("request body should encode");
    let capture = EventCaptureLayer::default();
    let subscriber = tracing_subscriber::registry().with(capture.clone());

    let response = router
        .oneshot(
            Request::post("/$procs/getFeedPage")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("accept", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "9")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .with_subscriber(subscriber)
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let joined = capture.snapshot().join("\n");
    assert!(joined.contains("cratestack procedure route completed"));
    assert!(joined.contains("cratestack procedure completed"));
    assert!(joined.contains("cratestack_route=/$procs/getFeedPage"));
}

#[tokio::test]
async fn single_output_procedure_route_rejects_cbor_sequence_accept_header() {
    let codec = CborCodec;
    let router = test_procedure_router(codec.clone());
    let body = codec
        .encode(&cratestack_schema::procedures::publish_post::Args {
            args: cratestack_schema::PublishPostInput { postId: 44 },
        })
        .expect("request body should encode");

    let response = router
        .oneshot(
            Request::post("/$procs/publishPost")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("accept", cratestack::CBOR_SEQUENCE_CONTENT_TYPE)
                .header("x-role", "admin")
                .header("x-auth-id", "9")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
}

mod custom_fields_schema {
    use self::cratestack_schema::CustomFieldResolver;
    use super::*;

    include_server_schema!("tests/fixtures/custom_fields.cstack", db = Postgres);

    #[derive(Clone)]
    struct TestCustomFieldResolver;

    impl cratestack_schema::CustomFieldResolver for TestCustomFieldResolver {
        fn resolve_image_thumbnail_url(
            &self,
            source: &cratestack_schema::Image,
            _ctx: &CoolContext,
        ) -> impl core::future::Future<Output = Result<String, cratestack::CoolError>> + Send
        {
            let storage_key = source.storageKey.clone();
            async move { Ok(format!("https://imgproxy.example/{storage_key}")) }
        }
    }

    #[test]
    fn macro_generates_custom_field_metadata() {
        assert_eq!(cratestack_schema::CUSTOM_FIELD_COUNT, 1);
        assert_eq!(cratestack_schema::CUSTOM_FIELDS[0].owner, "Image");
        assert_eq!(cratestack_schema::CUSTOM_FIELDS[0].field, "thumbnailUrl");
        assert_eq!(
            cratestack_schema::CUSTOM_FIELDS[0].resolver_method,
            "resolve_image_thumbnail_url"
        );
    }

    #[tokio::test]
    async fn generated_custom_field_resolver_trait_is_implementable() {
        let resolver = TestCustomFieldResolver;
        let image = cratestack_schema::Image {
            storageKey: "media/original.png".to_owned(),
            thumbnailUrl: "placeholder".to_owned(),
        };

        let resolved = resolver
            .resolve_image_thumbnail_url(&image, &CoolContext::anonymous())
            .await
            .expect("custom field should resolve");

        assert_eq!(resolved, "https://imgproxy.example/media/original.png");
    }
}

#[tokio::test]
async fn axum_procedure_route_denies_non_admin_invocation() {
    let codec = CborCodec;
    let router = test_procedure_router(codec.clone());
    let body = codec
        .encode(&cratestack_schema::procedures::publish_post::Args {
            args: cratestack_schema::PublishPostInput { postId: 44 },
        })
        .expect("request body should encode");

    let response = router
        .oneshot(
            Request::post("/$procs/publishPost")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-role", "member")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn axum_procedure_route_rejects_unsupported_content_type() {
    let codec = CborCodec;
    let router = test_procedure_router(codec.clone());
    let body = codec
        .encode(&cratestack_schema::procedures::publish_post::Args {
            args: cratestack_schema::PublishPostInput { postId: 44 },
        })
        .expect("request body should encode");

    let response = router
        .oneshot(
            Request::post("/$procs/publishPost")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn axum_model_route_rejects_negative_limit() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?limit=-1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_unacceptable_accept_header() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts")
                .header("accept", "application/json")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
}

#[tokio::test]
async fn axum_model_route_rejects_unknown_sort_field() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?sort=unknownField")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn axum_model_route_keeps_order_by_as_sort_compatibility_alias() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?orderBy=unknownField")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn axum_model_route_rejects_unknown_fields_selection() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?fields=id,unknownField")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn axum_model_route_rejects_unknown_include_selection() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?include=author,comments")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn axum_model_route_rejects_invalid_scalar_filter() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?authorId=not-an-int")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_invalid_uuid_filter() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/sessions?externalId=not-a-uuid")
                .header("x-auth-id", "1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_invalid_cuid_filter() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/sessions?id=not-a-cuid")
                .header("x-auth-id", "1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_invalid_datetime_filter() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/sessions?createdAt=not-a-datetime")
                .header("x-auth-id", "1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_unsupported_filter_operator() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?title__endsWith=raft")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_to_many_relation_filter_without_quantifier() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/users?sessions.label__contains=Revoked")
                .header("x-auth-id", "1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_unknown_to_many_relation_quantifier() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/users?sessions.any.label__contains=Revoked")
                .header("x-auth-id", "1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_to_many_relation_order_by() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/users?sort=sessions.label")
                .header("x-auth-id", "1")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn axum_model_route_rejects_malformed_nested_relation_filter_path() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?author..email=owner@example.com")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_invalid_nested_relation_order_by_path() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?sort=author.sessions.label")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn axum_model_route_rejects_malformed_or_group() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?or=title__startsWith")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_unterminated_where_group() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?where=(title__startsWith=Pub|published=true")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_unterminated_negated_where_group() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?where=not(title__startsWith=Pub|published=true")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_empty_negated_where_group() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?where=not()")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_route_rejects_negated_where_without_parentheses() {
    let codec = CborCodec;
    let router = test_model_router(codec);

    let response = router
        .oneshot(
            Request::get("/posts?where=not%20published=true")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_model_create_route_denies_anonymous_request_before_db_access() {
    let codec = CborCodec;
    let router = test_model_router(codec.clone());
    let body = codec
        .encode(&cratestack_schema::CreatePostInput {
            id: 9,
            title: "Draft".to_owned(),
            subtitle: None,
            published: false,
            authorId: 7,
        })
        .expect("request body should encode");

    let response = router
        .oneshot(
            Request::post("/posts")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn axum_model_create_route_rejects_missing_content_type() {
    let codec = CborCodec;
    let router = test_model_router(codec.clone());
    let body = codec
        .encode(&cratestack_schema::CreatePostInput {
            id: 9,
            title: "Draft".to_owned(),
            subtitle: None,
            published: false,
            authorId: 7,
        })
        .expect("request body should encode");

    let response = router
        .oneshot(
            Request::post("/posts")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn axum_model_update_route_rejects_empty_patch_before_db_access() {
    let codec = CborCodec;
    let router = test_model_router(codec.clone());
    let body = codec
        .encode(&cratestack_schema::UpdatePostInput::default())
        .expect("request body should encode");

    let response = router
        .oneshot(
            Request::patch("/posts/7")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-auth-id", "7")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn axum_combined_router_serves_procedure_routes() {
    let codec = CborCodec;
    let router = test_combined_router(codec.clone());
    let body = codec
        .encode(&cratestack_schema::procedures::publish_post::Args {
            args: cratestack_schema::PublishPostInput { postId: 31 },
        })
        .expect("request body should encode");

    let response = router
        .oneshot(
            Request::post("/$procs/publishPost")
                .header("content-type", CborCodec::CONTENT_TYPE)
                .header("x-role", "admin")
                .header("x-auth-id", "9")
                .body(Body::from(body))
                .expect("request should build"),
        )
        .await
        .expect("request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
}

// -----------------------------------------------------------------------------
// Transport-style introspection
//
// `transport rest` (the default) populates `ROUTE_TRANSPORTS` and leaves `OPS`
// empty. `transport rpc` does the opposite. See docs/design/rpc-transport.md.
// -----------------------------------------------------------------------------

#[test]
fn rest_schema_emits_route_transports_and_no_ops() {
    // The blog fixture omits the `transport` directive, so it picks up the
    // REST default.
    assert_eq!(cratestack_schema::TRANSPORT_STYLE, "rest");
    assert!(
        cratestack_schema::axum::OPS.is_empty(),
        "REST schemas must not populate the OPS slice; got {} entries",
        cratestack_schema::axum::OPS.len(),
    );
    assert!(
        !cratestack_schema::axum::ROUTE_TRANSPORTS.is_empty(),
        "REST schemas must populate ROUTE_TRANSPORTS",
    );
}

mod transport_rpc_schema {
    use super::*;

    include_server_schema!("tests/fixtures/transport_rpc.cstack", db = Postgres);

    #[test]
    fn rpc_schema_emits_ops_and_no_route_transports() {
        assert_eq!(cratestack_schema::TRANSPORT_STYLE, "rpc");
        assert!(
            cratestack_schema::axum::ROUTE_TRANSPORTS.is_empty(),
            "RPC schemas must not populate ROUTE_TRANSPORTS; got {} entries",
            cratestack_schema::axum::ROUTE_TRANSPORTS.len(),
        );
        assert!(
            !cratestack_schema::axum::OPS.is_empty(),
            "RPC schemas must populate the OPS slice",
        );
    }

    #[test]
    fn rpc_schema_emits_one_op_per_crud_verb_per_model() {
        let ops = cratestack_schema::axum::OPS;
        for verb in ["list", "get", "create", "update", "delete"] {
            let expected = format!("model.Widget.{verb}");
            assert!(
                ops.iter().any(|op| op.op_id == expected),
                "missing op_id `{expected}`; got: {:?}",
                ops.iter().map(|o| o.op_id).collect::<Vec<_>>(),
            );
        }
    }

    #[test]
    fn rpc_schema_op_kinds_match_procedure_shape() {
        let ops = cratestack_schema::axum::OPS;

        let ping = ops
            .iter()
            .find(|op| op.op_id == "procedure.ping")
            .expect("procedure.ping should be emitted");
        assert_eq!(ping.kind, cratestack::OpKind::Unary);
        assert!(
            ping.idempotent_by_default,
            "query procedures should be idempotent_by_default",
        );

        let bump = ops
            .iter()
            .find(|op| op.op_id == "procedure.bump")
            .expect("procedure.bump should be emitted");
        assert_eq!(bump.kind, cratestack::OpKind::Unary);
        assert!(
            !bump.idempotent_by_default,
            "mutation procedures should not be idempotent_by_default",
        );
    }

    #[test]
    fn rpc_schema_crud_idempotency_defaults_are_safe() {
        let ops = cratestack_schema::axum::OPS;
        for op in ops {
            match op.op_id {
                "model.Widget.list" | "model.Widget.get" => {
                    assert!(op.idempotent_by_default, "{} should be idempotent", op.op_id)
                }
                "model.Widget.create" | "model.Widget.update" | "model.Widget.delete" => {
                    assert!(
                        !op.idempotent_by_default,
                        "{} must not default to idempotent (writes)",
                        op.op_id,
                    )
                }
                _ => {}
            }
        }
    }

    #[test]
    fn rpc_schema_crud_input_and_output_types_use_generated_names() {
        let ops = cratestack_schema::axum::OPS;
        let by_id = |id: &str| {
            ops.iter()
                .find(|op| op.op_id == id)
                .unwrap_or_else(|| panic!("missing op {id}"))
        };

        assert_eq!(by_id("model.Widget.list").output_ty, "Page<Widget>");
        assert_eq!(by_id("model.Widget.get").output_ty, "Widget");
        assert_eq!(by_id("model.Widget.create").input_ty, "CreateWidgetInput");
        assert_eq!(by_id("model.Widget.update").input_ty, "UpdateWidgetInput");
    }

    // -------------------------------------------------------------------------
    // RPC unary runtime: procedure dispatch
    //
    // The macro emits an `rpc_router` (gated on `transport rpc`) that mounts
    // `POST /rpc/{op_id}`. Procedure ops dispatch into the existing
    // `handle_<name>` axum handler; model CRUD ops return 501 for now (next
    // patch wires them).
    // -------------------------------------------------------------------------

    #[derive(Clone)]
    struct RpcTestProcedures;

    impl cratestack_schema::procedures::ProcedureRegistry for RpcTestProcedures {
        fn ping(
            &self,
            _db: &cratestack_schema::Cratestack,
            _ctx: &CoolContext,
            args: cratestack_schema::procedures::ping::Args,
        ) -> impl core::future::Future<
            Output = Result<cratestack_schema::procedures::ping::Output, cratestack::CoolError>,
        > + Send {
            async move { Ok(args.args) }
        }

        fn bump(
            &self,
            _db: &cratestack_schema::Cratestack,
            _ctx: &CoolContext,
            args: cratestack_schema::procedures::bump::Args,
        ) -> impl core::future::Future<
            Output = Result<cratestack_schema::procedures::bump::Output, cratestack::CoolError>,
        > + Send {
            async move {
                Ok(cratestack_schema::PingArgs {
                    nonce: format!("{}!", args.args.nonce),
                })
            }
        }
    }

    /// Auth provider for the RPC runtime tests. Returns an authenticated
    /// context whenever an `x-auth-id` header is present; anonymous
    /// otherwise. The fixture's procedures use `@allow(auth() != null)`
    /// so tests opt in by sending the header.
    #[derive(Clone)]
    struct RpcTestAuthProvider;

    impl AuthProvider for RpcTestAuthProvider {
        type Error = cratestack::CoolError;

        fn authenticate(
            &self,
            request: &RequestContext<'_>,
        ) -> impl core::future::Future<Output = Result<CoolContext, Self::Error>> + Send {
            let ctx = request
                .headers
                .get("x-auth-id")
                .and_then(|value| value.to_str().ok())
                .and_then(|raw| raw.parse::<i64>().ok())
                .map(|id| CoolContext::authenticated([("id".to_owned(), Value::Int(id))]))
                .unwrap_or_else(CoolContext::anonymous);
            core::future::ready(Ok(ctx))
        }
    }

    fn rpc_test_db() -> cratestack_schema::Cratestack {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://cratestack:cratestack@localhost/cratestack")
            .expect("lazy pool should parse");
        cratestack_schema::Cratestack::builder(pool).build()
    }

    fn rpc_test_router(codec: CborCodec) -> cratestack::axum::Router {
        cratestack_schema::axum::rpc_router(
            rpc_test_db(),
            RpcTestProcedures,
            codec,
            RpcTestAuthProvider,
        )
    }

    #[tokio::test]
    async fn rpc_unary_dispatches_query_procedure() {
        let codec = CborCodec;
        let router = rpc_test_router(codec.clone());
        let body = codec
            .encode(&cratestack_schema::procedures::ping::Args {
                args: cratestack_schema::PingArgs {
                    nonce: "hello".to_owned(),
                },
            })
            .expect("ping request should encode");

        let response = router
            .oneshot(
                Request::post("/rpc/procedure.ping")
                    .header("content-type", CborCodec::CONTENT_TYPE)
                    .header("x-auth-id", "1")
                    .body(Body::from(body))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK);

        let response_bytes = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should buffer");
        let decoded: cratestack_schema::PingArgs = codec
            .decode(&response_bytes)
            .expect("response should decode as PingArgs");
        assert_eq!(decoded.nonce, "hello");
    }

    #[tokio::test]
    async fn rpc_unary_dispatches_mutation_procedure() {
        let codec = CborCodec;
        let router = rpc_test_router(codec.clone());
        let body = codec
            .encode(&cratestack_schema::procedures::bump::Args {
                args: cratestack_schema::PingArgs {
                    nonce: "x".to_owned(),
                },
            })
            .expect("bump request should encode");

        let response = router
            .oneshot(
                Request::post("/rpc/procedure.bump")
                    .header("content-type", CborCodec::CONTENT_TYPE)
                    .header("x-auth-id", "1")
                    .body(Body::from(body))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::OK);
        let response_bytes = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should buffer");
        let decoded: cratestack_schema::PingArgs = codec
            .decode(&response_bytes)
            .expect("response should decode as PingArgs");
        assert_eq!(decoded.nonce, "x!");
    }

    /// Build a CBOR body of a value that's serializable. Lifts the
    /// boilerplate of unwrapping codec.encode out of the CRUD tests below.
    fn cbor(value: &impl serde::Serialize) -> Vec<u8> {
        CborCodec
            .encode(value)
            .expect("test body should encode")
    }

    /// Build an RPC unary request with CBOR content-type + auth header.
    fn rpc_request(op_id: &str, body: Vec<u8>) -> cratestack::axum::http::Request<Body> {
        Request::post(format!("/rpc/{op_id}"))
            .header("content-type", CborCodec::CONTENT_TYPE)
            .header("x-auth-id", "1")
            .body(Body::from(body))
            .expect("request should build")
    }

    #[tokio::test]
    async fn rpc_unary_create_rejects_malformed_body() {
        // Wrong-shape body (missing required `name`) — the existing
        // create handler should reject this with a 4xx before ever
        // hitting the DB. Validates that dispatch routes to handle_create
        // and that the handler's decode path is reached.
        let router = rpc_test_router(CborCodec);
        let body = cbor(&serde_json::json!({}));
        let response = router
            .oneshot(rpc_request("model.Widget.create", body))
            .await
            .expect("request should succeed");
        assert!(
            response.status().is_client_error(),
            "malformed create body should be 4xx, got {}",
            response.status(),
        );
    }

    #[tokio::test]
    async fn rpc_unary_get_returns_4xx_on_unparseable_pk() {
        // `Widget.id` is `Int`; sending a string instead exercises the
        // RpcPkInput<i32> decode path inside the dispatcher. The decode
        // error surfaces as a 4xx via `rpc_dispatch_error`, with no DB
        // involvement.
        let router = rpc_test_router(CborCodec);
        let body = cbor(&serde_json::json!({"id": "not-a-number"}));
        let response = router
            .oneshot(rpc_request("model.Widget.get", body))
            .await
            .expect("request should succeed");
        assert!(
            response.status().is_client_error(),
            "non-integer id should be 4xx, got {}",
            response.status(),
        );
    }

    #[tokio::test]
    async fn rpc_unary_delete_returns_4xx_on_unparseable_pk() {
        let router = rpc_test_router(CborCodec);
        let body = cbor(&serde_json::json!({"id": "not-a-number"}));
        let response = router
            .oneshot(rpc_request("model.Widget.delete", body))
            .await
            .expect("request should succeed");
        assert!(
            response.status().is_client_error(),
            "non-integer id should be 4xx, got {}",
            response.status(),
        );
    }

    #[tokio::test]
    async fn rpc_unary_update_returns_4xx_on_malformed_patch() {
        // Well-formed id, malformed patch (an invalid field type for
        // `name`). The dispatcher decodes RpcUpdateInput<i32, UpdateWidgetInput>
        // and rejects before re-encoding.
        let router = rpc_test_router(CborCodec);
        let body = cbor(&serde_json::json!({
            "id": 1,
            "patch": { "name": 42 }
        }));
        let response = router
            .oneshot(rpc_request("model.Widget.update", body))
            .await
            .expect("request should succeed");
        assert!(
            response.status().is_client_error(),
            "type-mismatched patch should be 4xx, got {}",
            response.status(),
        );
    }

    #[tokio::test]
    async fn rpc_unary_list_accepts_pagination_input_shape() {
        // Body decodes as RpcListInput, gets synthesized into a query
        // string, gets parsed back by `parse_model_list_query`. If the
        // round-trip is broken the handler returns 4xx; this test asserts
        // we get past that — the only error left is the DB failure (no
        // postgres in the test env) which surfaces as 5xx.
        let router = rpc_test_router(CborCodec);
        let body = cbor(&serde_json::json!({
            "limit": 5,
            "offset": 10,
        }));
        let response = router
            .oneshot(rpc_request("model.Widget.list", body))
            .await
            .expect("request should succeed");
        assert!(
            response.status().is_server_error()
                || response.status() == StatusCode::FORBIDDEN,
            "list pagination should reach the handler (forbidden by policy or DB error), got {}",
            response.status(),
        );
    }

    #[tokio::test]
    async fn rpc_unary_list_rejects_malformed_input_shape() {
        // `limit` must be an integer — sending a string is a decode
        // error inside the dispatcher, surfaces as 4xx.
        let router = rpc_test_router(CborCodec);
        let body = cbor(&serde_json::json!({
            "limit": "five",
        }));
        let response = router
            .oneshot(rpc_request("model.Widget.list", body))
            .await
            .expect("request should succeed");
        assert!(
            response.status().is_client_error(),
            "non-integer limit should be 4xx, got {}",
            response.status(),
        );
    }

    // ----- batch -----

    fn batch_request(frames: Vec<cratestack::rpc::RpcRequest>) -> cratestack::axum::http::Request<Body> {
        let body = CborCodec
            .encode(&frames)
            .expect("batch body should encode");
        Request::post("/rpc/batch")
            .header("content-type", CborCodec::CONTENT_TYPE)
            .header("x-auth-id", "1")
            .body(Body::from(body))
            .expect("request should build")
    }

    async fn run_batch(
        router: cratestack::axum::Router,
        frames: Vec<cratestack::rpc::RpcRequest>,
    ) -> (StatusCode, Vec<cratestack::rpc::RpcResponseFrame>) {
        let response = router
            .oneshot(batch_request(frames))
            .await
            .expect("batch request should succeed");
        let status = response.status();
        let bytes = cratestack::axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should buffer");
        let decoded: Vec<cratestack::rpc::RpcResponseFrame> = CborCodec
            .decode(&bytes)
            .expect("batch response should decode as Vec<RpcResponseFrame>");
        (status, decoded)
    }

    #[tokio::test]
    async fn rpc_batch_preserves_response_order_and_correlates_ids() {
        let router = rpc_test_router(CborCodec);
        let frames = vec![
            cratestack::rpc::RpcRequest {
                id: 100,
                op: "procedure.ping".into(),
                input: serde_json::json!({
                    "args": { "nonce": "first" }
                }),
                idem: None,
            },
            cratestack::rpc::RpcRequest {
                id: 200,
                op: "procedure.bump".into(),
                input: serde_json::json!({
                    "args": { "nonce": "second" }
                }),
                idem: None,
            },
        ];

        let (status, responses) = run_batch(router, frames).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0].id, 100);
        assert_eq!(responses[1].id, 200);
        assert!(responses[0].error.is_none(), "frame 0 should succeed: {:?}", responses[0]);
        assert!(responses[1].error.is_none(), "frame 1 should succeed: {:?}", responses[1]);

        let out0 = responses[0].output.as_ref().expect("ok frame has output");
        assert_eq!(out0.get("nonce").and_then(|v| v.as_str()), Some("first"));

        let out1 = responses[1].output.as_ref().expect("ok frame has output");
        assert_eq!(out1.get("nonce").and_then(|v| v.as_str()), Some("second!"));
    }

    #[tokio::test]
    async fn rpc_batch_per_frame_errors_dont_poison_other_frames() {
        // Mix one valid procedure call with one unknown op; the batch
        // still returns 200, the valid frame succeeds, the bad frame
        // carries an error.
        let router = rpc_test_router(CborCodec);
        let frames = vec![
            cratestack::rpc::RpcRequest {
                id: 1,
                op: "procedure.ping".into(),
                input: serde_json::json!({"args": {"nonce": "ok"}}),
                idem: None,
            },
            cratestack::rpc::RpcRequest {
                id: 2,
                op: "procedure.does_not_exist".into(),
                input: serde_json::json!(null),
                idem: None,
            },
        ];

        let (status, responses) = run_batch(router, frames).await;

        assert_eq!(status, StatusCode::OK, "batch envelope must succeed");
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0].id, 1);
        assert_eq!(responses[1].id, 2);
        assert!(responses[0].error.is_none(), "frame 1 should succeed");
        assert!(
            responses[1].error.is_some(),
            "frame 2 (unknown op) should carry an error: {:?}",
            responses[1],
        );
    }

    #[tokio::test]
    async fn rpc_batch_malformed_envelope_returns_4xx() {
        // Body that isn't a sequence of RpcRequest frames — should
        // surface as a 4xx, NOT a 200 with an empty array.
        let router = rpc_test_router(CborCodec);
        let body = CborCodec
            .encode(&serde_json::json!({"not": "a sequence"}))
            .expect("body should encode");
        let response = router
            .oneshot(
                Request::post("/rpc/batch")
                    .header("content-type", CborCodec::CONTENT_TYPE)
                    .header("x-auth-id", "1")
                    .body(Body::from(body))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
        assert!(
            response.status().is_client_error(),
            "malformed batch envelope should be 4xx, got {}",
            response.status(),
        );
    }

    #[tokio::test]
    async fn rpc_batch_rejects_idempotency_key_header() {
        // Per-frame idempotency is the model; the HTTP header is
        // ambiguous in batch context and explicitly rejected.
        let router = rpc_test_router(CborCodec);
        let body = CborCodec
            .encode(&Vec::<cratestack::rpc::RpcRequest>::new())
            .expect("body should encode");
        let response = router
            .oneshot(
                Request::post("/rpc/batch")
                    .header("content-type", CborCodec::CONTENT_TYPE)
                    .header("x-auth-id", "1")
                    .header("idempotency-key", "abc-123")
                    .body(Body::from(body))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn rpc_batch_empty_returns_empty_response() {
        // No frames in, no frames out. Doesn't 400, doesn't crash.
        let router = rpc_test_router(CborCodec);
        let (status, responses) = run_batch(router, Vec::new()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(responses.is_empty());
    }

    #[tokio::test]
    async fn rpc_unary_unknown_op_returns_404() {
        let codec = CborCodec;
        let router = rpc_test_router(codec);
        let response = router
            .oneshot(
                Request::post("/rpc/procedure.does_not_exist")
                    .header("content-type", CborCodec::CONTENT_TYPE)
                    .body(Body::from(Vec::<u8>::new()))
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

}
