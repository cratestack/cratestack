use cratestack_core::Schema;

use crate::diagnostics::SchemaError;

use crate::directives::FileDirectives;

/// One file's declarations plus its multi-file directives. Mixins are not
/// expanded yet: the loader merges every file first, so a model can `@use` a
/// mixin declared in another file.
pub(crate) struct ParsedFile {
    pub(crate) schema: Schema,
    pub(crate) directives: FileDirectives,
}

/// Expand `@use` mixins into the models of `schema`, once every file of it
/// has been merged.
pub(crate) fn expand_mixins(schema: &mut Schema) -> Result<(), SchemaError> {
    let mixins = std::mem::take(&mut schema.mixins);
    let result = super::models::expand_model_mixins(&mixins, &mut schema.models);
    schema.mixins = mixins;
    result
}
