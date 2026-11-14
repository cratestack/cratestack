import 'dart:convert';
import 'dart:typed_data';

/// Hex-encoded SHA-256 of the `.cstack` schema source this client was
/// generated from (issue #178). Sent as `x-cratestack-schema-sha` on
/// every outgoing request so the server-side drift-detection middleware
/// (`cratestack-axum::schema_fingerprint`) can warn when a client and
/// server were generated from different copies of the schema — it never
/// rejects a request, only logs. `null` when the generator wasn't given
/// a schema hash (e.g. this crate used as a library directly, bypassing
/// `cratestack generate-dart`), in which case every runtime adapter
/// omits the header entirely rather than sending an empty value.
// The nullable type below is structurally correct across the space of
// possible generator configs (see the doc comment above) even though it's
// provably non-null for this one generated instance — narrowing it would
// break the no-hash case, which is a real, exercised code path.
// ignore: unnecessary_nullable_for_final_variable_declarations
const String? cratestackSchemaSha256 = '9f1c1e3b6b7f27e0d2a5b1c4e8f0a3d6c9b2e5f8a1d4c7b0e3f6a9c2d5b8e1f4';

/// Per-op contract digests (binding version 2, cratestack#1123): what a
/// signed request to each op binds into its COSE AAD, keyed like the Rust
/// client's `OP_CONTRACTS` (the RPC op id, or `<METHOD> <route template>` on
/// REST, plus `batch` for `transport rpc`), as lowercase hex. Computed by the
/// same `cratestack_core` function the Rust macros call, so the two cannot
/// disagree. The unsigned runtime does not read them; a sealer does.
const Map<String, String> cratestackOpContracts = <String, String>{
  'batch': '57a7edd93eb8336bc5ca6b82db2ac5984f6e50129dc2933f4068346ef6ea2538',
  'procedure.beginEnrolment': '856ae118b541ca5afd32fa0425be1a1cee3ecf1169d499cb1850b7d7e5f1d4ee',
  'procedure.beginRenewal': 'cd6e5940e155d7e22ace0cdad9577dde996ddebe8428c3e718b2b89b0fd9fadb',
  'procedure.confirmEnrolment': '855546e96ea3063fc9f9d543ae86dadf4586d536d486e35f159f9014a89ec386',
  'procedure.confirmRenewal': '1b1fec5dc3ac2ebf59b42e5580654c474751938f40ed6d366de3023c19f51e18',
  'procedure.revokeDevice': 'c10fb1e4b628eb5ea0f207be8dfcad611ab6adc8714e3be611cc91289a442d9e',
  'procedure.sendEnrolmentCode': '92e831f5f8d3291500ce27d49528d5de652a62d79f860c21ab48e18f29da70d8',
};

/// Hex of the whole-contract digest: moves when any op's contract does.
const String cratestackClientContractSha256 = '57a7edd93eb8336bc5ca6b82db2ac5984f6e50129dc2933f4068346ef6ea2538';

/// The compact wire key tables of every op (`docs/design/compact-wire-keys.md`),
/// the artifact `createCompactKeys` of `package:cratestack_cbor` reads. Only a
/// client that opts into compact keys touches it:
///
/// ```dart
/// final keys = await createCompactKeys(cratestackKeyTables);
/// final hooks = CratestackCompactHooks(
///   toCompact: keys.toCompact,
///   fromCompact: keys.fromCompact,
///   selector: keys.selector,
/// );
/// ```
final Uint8List cratestackKeyTables = base64Decode(
  'gwGTiGdhbWJpZW50A2NvdHAAZXBob25lAGt1c2VyUHJlc2VudAOGZ2FtYmllbnQAa2Vucm9sbWVudElkAGt1c2VyUHJlc2VudACIY2NydgBja3R5AGF4AGF5AIJkYXJncwGEaGRldmljZUlkAGRyb2xlAIRpY2hhbGxlbmdlAGtjaGFsbGVuZ2VJZACCZGFyZ3MFhmdhbWJpZW50AGtlbnJvbG1lbnRJZABrdXNlclByZXNlbnQAimdhbWJpZW50AGhkZXZpY2VJZABlcGhvbmUAZnVzZXJJZABrdXNlclByZXNlbnQAgmRhcmdzCIRrY2hhbGxlbmdlSWQAaXNpZ25hdHVyZQCCa2F0dGVzdGF0aW9uAIJkYXJncwuCaGRldmljZUlkAIJoZGV2aWNlSWQAgmRhcmdzDoJlcGhvbmUAgmVwaG9uZQCCZGFyZ3MShoR4GHByb2NlZHVyZS5iZWdpbkVucm9sbWVudFgghWrhGLVBylr9MvoEJb4aHO4+zxFp1JnLGFC31+Xx1O4EAoR2cHJvY2VkdXJlLmJlZ2luUmVuZXdhbFggzW5ZQOFV1+Iqzgza2Vd93plt3r6EKMPnGLK4mw/Z+tsHBoR4GnByb2NlZHVyZS5jb25maXJtRW5yb2xtZW50WCCFVUbpbqMGP8n51UOuhtrfRYbVNtSG418Vn5AUqJ7DhgoJhHgYcHJvY2VkdXJlLmNvbmZpcm1SZW5ld2FsWCAbH+xdw6wuv1m0LlWAZUxHR1GTj0DtbTZt4wI8GfUeGA0MhHZwcm9jZWR1cmUucmV2b2tlRGV2aWNlWCDBD7HktijrXqDyB76N/K1hGratyHFOO+YRzJEomkQtnhAPhHgbcHJvY2VkdXJlLnNlbmRFbnJvbG1lbnRDb2RlWCCS6DH1+NMpFQDOJ9SVKNXeZSpi15+GDCGrSOGPKdpw2BMR',
);

