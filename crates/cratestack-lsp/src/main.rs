mod analyze;
mod completion;
mod definition;
mod document_symbols;
mod hover;
mod relation_parse;
mod server;
mod state;
mod text;
mod type_ref;

#[cfg(test)]
mod tests;

use state::Backend;
use tower_lsp_server::{LspService, Server};

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
