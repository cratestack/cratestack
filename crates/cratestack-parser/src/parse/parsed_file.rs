use cratestack_core::Schema;

use crate::directives::FileDirectives;

/// One file's declarations plus its multi-file directives. Mixins are not
/// expanded yet: the loader merges every file first, so a model can `@use` a
/// mixin declared in another file.
pub(crate) struct ParsedFile {
    pub(crate) schema: Schema,
    pub(crate) directives: FileDirectives,
}
