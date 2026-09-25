//! `mcp-operator-example <stdio | http | mint-token>`; see README.md.
//!
//! Every mode reads the signing key from `MCP_EXAMPLE_SIGNING_KEY`, and
//! there is no built-in default: a demo key compiled into the binary would
//! be a key anyone can mint an editor's token with.

use std::process::ExitCode;

use cratestack::mcp::StdioServer;
use cratestack::sqlx::PgPool;
use mcp_operator_example::http::{HttpConfig, app};
use mcp_operator_example::token::{STDIO_AUDIENCE, TokenVerifier, mint};
use mcp_operator_example::{ensure_schema, mcp_table, schema};

const USAGE: &str = "usage:
  mcp-operator-example stdio
      env: DATABASE_URL, MCP_EXAMPLE_SIGNING_KEY, MCP_EXAMPLE_TOKEN (audience cratestack://blog)
  mcp-operator-example http [--addr 127.0.0.1:8787] [--resource URL] [--allowed-origin ORIGIN]...
      env: DATABASE_URL, MCP_EXAMPLE_SIGNING_KEY
  mcp-operator-example mint-token --audience AUD --id ID --role ROLE [--ttl SECONDS]
      env: MCP_EXAMPLE_SIGNING_KEY";

const DEFAULT_DATABASE_URL: &str =
    "postgres://cratestack:cratestack@localhost:55432/cratestack_test";

#[tokio::main]
async fn main() -> ExitCode {
    // stderr only: over stdio, stdout is the protocol stream.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("stdio") => stdio().await,
        Some("http") => http(&args[1..]).await,
        Some("mint-token") => mint_token(&args[1..]),
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// stdio: the spec says credentials come from the environment, so the
/// caller is whoever `MCP_EXAMPLE_TOKEN` verifies as. No token, no server:
/// there is no default identity (ADR 0002 Q1).
async fn stdio() -> Result<(), String> {
    let token = env("MCP_EXAMPLE_TOKEN")?;
    let ctx = TokenVerifier::new(&signing_key()?, STDIO_AUDIENCE)?
        .verify(&token)
        .map_err(|error| format!("MCP_EXAMPLE_TOKEN refused: {error}"))?;
    let db = database().await?;
    StdioServer::new(mcp_table(db), ctx)
        .map_err(|error| error.to_string())?
        .serve()
        .await
        .map_err(|error| error.to_string())
}

async fn http(args: &[String]) -> Result<(), String> {
    let addr = flag(args, "--addr").unwrap_or_else(|| "127.0.0.1:8787".to_owned());
    let resource = flag(args, "--resource").unwrap_or_else(|| format!("http://{addr}/mcp"));
    let mut allowed_origins = flags(args, "--allowed-origin");
    if allowed_origins.is_empty() {
        // The MCP Inspector web UI's default origin.
        allowed_origins.push("http://localhost:6274".to_owned());
    }
    let config = HttpConfig {
        resource,
        allowed_origins,
        key: signing_key()?,
    };
    let router = app(database().await?, &config)?;
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|error| format!("bind {addr}: {error}"))?;
    eprintln!("MCP over Streamable HTTP at {}", config.resource);
    eprintln!("token audience must be exactly {}", config.resource);
    cratestack::axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|error| error.to_string())
}

fn mint_token(args: &[String]) -> Result<(), String> {
    let required = |name: &str| flag(args, name).ok_or_else(|| format!("{name} is required"));
    let ttl = match flag(args, "--ttl") {
        Some(ttl) => ttl
            .parse()
            .map_err(|_| "--ttl must be seconds".to_owned())?,
        None => 3600,
    };
    let token = mint(
        &signing_key()?,
        &required("--audience")?,
        &required("--id")?,
        &required("--role")?,
        ttl,
    );
    println!("{token}");
    Ok(())
}

async fn database() -> Result<schema::Cratestack, String> {
    let url = std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
    let pool = PgPool::connect(&url)
        .await
        .map_err(|error| format!("connect to Postgres (DATABASE_URL): {error}"))?;
    ensure_schema(&pool)
        .await
        .map_err(|error| format!("create and seed `posts`: {error}"))?;
    Ok(schema::Cratestack::builder(pool).build())
}

fn signing_key() -> Result<Vec<u8>, String> {
    env("MCP_EXAMPLE_SIGNING_KEY").map(String::into_bytes)
}

fn env(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is not set\n\n{USAGE}"))
}

fn flag(args: &[String], name: &str) -> Option<String> {
    flags(args, name).pop()
}

fn flags(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
        .collect()
}
