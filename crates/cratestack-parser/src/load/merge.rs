//! Merging the parsed files into one `Schema`, in load order
//! (docs/design/multi-file-schemas.md section 7).

use std::collections::BTreeSet;

use cratestack_core::Schema;

use crate::parse::ParsedFile;

/// Appends every list in load order and unions the extensions. The
/// singletons (`datasource`, `auth`, `mcp`, `transport`) come from the first
/// file, the root: a part cannot declare one (M5). Mixins are expanded by the
/// caller afterwards, so a model can `@use` a mixin from another file.
///
/// `Schema` is destructured exhaustively, so a new field fails to compile
/// here until it is merged on purpose.
pub(super) fn merge(files: Vec<ParsedFile>) -> Schema {
    let mut merged = Schema {
        datasource: None,
        auth: None,
        config_blocks: Vec::new(),
        mixins: Vec::new(),
        models: Vec::new(),
        types: Vec::new(),
        enums: Vec::new(),
        procedures: Vec::new(),
        views: Vec::new(),
        queries: Vec::new(),
        subscription_procedures: Vec::new(),
        topics: Vec::new(),
        transport: Default::default(),
        declared_extensions: BTreeSet::new(),
        mcp: None,
    };
    for (index, file) in files.into_iter().enumerate() {
        let Schema {
            datasource,
            auth,
            config_blocks,
            mixins,
            models,
            types,
            enums,
            procedures,
            views,
            queries,
            subscription_procedures,
            topics,
            transport,
            declared_extensions,
            mcp,
        } = file.schema;
        if index == 0 {
            merged.datasource = datasource;
            merged.auth = auth;
            merged.transport = transport;
            merged.mcp = mcp;
        }
        merged.config_blocks.extend(config_blocks);
        merged.mixins.extend(mixins);
        merged.models.extend(models);
        merged.types.extend(types);
        merged.enums.extend(enums);
        merged.procedures.extend(procedures);
        merged.views.extend(views);
        merged.queries.extend(queries);
        merged
            .subscription_procedures
            .extend(subscription_procedures);
        merged.topics.extend(topics);
        merged.declared_extensions.extend(declared_extensions);
    }
    merged
}
