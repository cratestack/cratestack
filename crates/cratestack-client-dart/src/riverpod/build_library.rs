//! Builds `lib/<package_name>.dart` — the riverpod preset's library
//! entrypoint. Exports every per-model file (plus the shared/procedures/
//! client files) so a consumer that wants one `import` still gets one,
//! matching issue #301's acceptance criterion for the entrypoint.
use cratestack_core::Schema;

use crate::riverpod::imports::model_file_path;
use crate::riverpod::views::LibraryFileContext;

pub(crate) fn build_library_file(schema: &Schema, is_rest: bool) -> LibraryFileContext {
    let mut exports = vec!["export 'src/runtime.dart';".to_owned()];
    if is_rest {
        exports.push("export 'src/queries.dart';".to_owned());
    }
    exports.push("export 'src/constants.dart';".to_owned());
    exports.push("export 'src/models/shared_types.dart';".to_owned());
    for model in &schema.models {
        exports.push(format!(
            "export 'src/models/{}';",
            model_file_path(&model.name)
        ));
    }
    exports.push("export 'src/procedures.dart';".to_owned());
    exports.push("export 'src/client.dart';".to_owned());

    LibraryFileContext { exports }
}
