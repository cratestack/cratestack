mod analyze;
mod attribute_completion;
mod authn_completion;
mod capabilities;
mod completion;
mod definition;
mod definition_graph;
mod document_symbols;
mod field_host;
mod graph;
mod graph_analyse;
mod graph_close;
mod graph_resolver;
mod graph_store;
mod graph_update;
mod hover;
mod hover_render;
mod importers;
mod mcp_completion;
mod mcp_hover;
mod mcp_tokens;
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
mod symbol_build;
mod symbol_target;
mod text;
mod token_encode;
mod type_hover;
mod type_ref;
mod view;
mod workspace_roots;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_attribute_run_diagnostics;
#[cfg(test)]
mod tests_block_diagnostics;
#[cfg(test)]
mod tests_completion;
#[cfg(test)]
mod tests_enum_hover;
#[cfg(test)]
mod tests_graph_analysis;
#[cfg(test)]
mod tests_graph_close;
#[cfg(test)]
mod tests_graph_definition;
#[cfg(test)]
mod tests_graph_nav_support;
#[cfg(test)]
mod tests_graph_navigation;
#[cfg(test)]
mod tests_graph_refresh;
#[cfg(test)]
mod tests_graph_support;
#[cfg(test)]
mod tests_graph_uris;
#[cfg(test)]
mod tests_last_known_good;
#[cfg(test)]
mod tests_mcp;
#[cfg(test)]
mod tests_navigation;
#[cfg(test)]
mod tests_queries;
#[cfg(test)]
mod tests_queries_rename;
#[cfg(test)]
mod tests_rename;
#[cfg(test)]
mod tests_rename_directives;
#[cfg(test)]
mod tests_rename_importers;
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
