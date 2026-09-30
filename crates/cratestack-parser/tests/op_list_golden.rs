//! cratestack#1123: the op list, pinned as literals. The keys are what a
//! signed client keys its per-op contract by, so they must not move with a
//! refactor: each schema below lists every op it exposes, written out.
//! Covers `@@internal`, `@@subscribe`, a sequence procedure, `@api_version`,
//! an underscored model name and both transports.

use cratestack_core::op_keys;

const HEAD: &str = "datasource db {\n  provider = \"postgresql\"\n  url = env(\"DATABASE_URL\")\n}\n\nauth Operator {\n  id Int\n}\n";

const MODELS: &str = r#"
model Widget {
  id Int @id
  name String

  @@allow("read", auth() != null)
  @@internal("create")
  @@emit(created)
  @@SUBSCRIBE@@
}

model Gadget {
  id Int @id

  @@allow("read", auth() != null)
  @@internal("all")
}

model Bank_Account {
  id Int @id
  label String

  @@allow("read", auth() != null)
}

type PingArgs {
  message String
}

procedure ping(args: PingArgs): PingArgs
  @api_version("v2")
  @allow(auth() != null)

mutation procedure bump(args: PingArgs): PingArgs
  @allow(auth() != null)

procedure many(args: PingArgs): PingArgs[]
  @allow(auth() != null)
"#;

fn keys(transport: &str, subscribe: &str) -> Vec<String> {
    let source = format!(
        "{transport}{HEAD}{}",
        MODELS.replace("@@SUBSCRIBE@@", subscribe)
    );
    op_keys(&cratestack_parser::parse_schema(&source).expect("schema parses"))
}

#[test]
fn rpc_ops() {
    assert_eq!(
        keys("transport rpc\n", "@@subscribe"),
        [
            "model.Bank_Account.create",
            "model.Bank_Account.delete",
            "model.Bank_Account.get",
            "model.Bank_Account.list",
            "model.Bank_Account.update",
            "model.Widget.delete",
            "model.Widget.get",
            "model.Widget.list",
            "model.Widget.subscribe",
            "model.Widget.update",
            "procedure.bump",
            "procedure.many",
            "procedure.ping",
        ]
    );
}

#[test]
fn rest_ops_have_no_subscribe_and_carry_the_api_version() {
    assert_eq!(
        keys("", "@@deny(\"read\", false)"),
        [
            "DELETE /bank__accounts/{id}",
            "DELETE /widgets/{id}",
            "GET /bank__accounts",
            "GET /bank__accounts/{id}",
            "GET /widgets",
            "GET /widgets/{id}",
            "PATCH /bank__accounts/{id}",
            "PATCH /widgets/{id}",
            "POST /$procs/bump",
            "POST /$procs/many",
            "POST /bank__accounts",
            "POST /v2/$procs/ping",
        ]
    );
}
