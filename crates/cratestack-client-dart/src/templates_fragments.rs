//! Include-only fragments. Registered with the minijinja environment so
//! `{% include %}` resolves, but not rendered to disk — they're slices of
//! larger templates that exceed the workspace ≤200-LoC rule.

pub(super) const FRAGMENT_TEMPLATES: &[(&str, &str)] = &[
    (
        "readme/setup.md.j2",
        include_str!("../templates/readme/setup.md.j2"),
    ),
    (
        "readme/flutter_usage.md.j2",
        include_str!("../templates/readme/flutter_usage.md.j2"),
    ),
    (
        "readme/paged.md.j2",
        include_str!("../templates/readme/paged.md.j2"),
    ),
    (
        "readme/crud_procedures.md.j2",
        include_str!("../templates/readme/crud_procedures.md.j2"),
    ),
    (
        "readme/queries.md.j2",
        include_str!("../templates/readme/queries.md.j2"),
    ),
    (
        "readme/api_bridge.md.j2",
        include_str!("../templates/readme/api_bridge.md.j2"),
    ),
    (
        "rpc_runtime/types.dart.j2",
        include_str!("../templates/rpc_runtime/types.dart.j2"),
    ),
    (
        "rpc_runtime/dio_json.dart.j2",
        include_str!("../templates/rpc_runtime/dio_json.dart.j2"),
    ),
    (
        "rpc_runtime/dio_cbor.dart.j2",
        include_str!("../templates/rpc_runtime/dio_cbor.dart.j2"),
    ),
    (
        "rpc_runtime/cbor_seq.dart.j2",
        include_str!("../templates/rpc_runtime/cbor_seq.dart.j2"),
    ),
];
