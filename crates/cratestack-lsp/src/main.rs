mod analyze;
mod capabilities;
mod completion;
mod definition;
mod document_symbols;
mod hover;
mod hover_render;
mod mixin_use;
mod navigation;
mod query_symbols;
mod references;
mod relation_parse;
mod rename;
mod rename_error;
mod semantic_tokens;
mod server;
mod state;
mod symbol_target;
mod text;
mod type_ref;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_last_known_good;
#[cfg(test)]
mod tests_navigation;
#[cfg(test)]
mod tests_queries;
#[cfg(test)]
mod tests_rename;
#[cfg(test)]
mod tests_semantic_tokens;

use state::Backend;
use tower_lsp_server::{LspService, Server};

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
