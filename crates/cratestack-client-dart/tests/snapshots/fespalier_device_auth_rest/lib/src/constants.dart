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
  'POST /\$procs/beginEnrolment': '017317cfacc5f165ef56b4476e8af5b938ab4619a4ca6a03a9eee818f1e53961',
  'POST /\$procs/beginRenewal': 'ef4e94aecb35432a6b5a1c332c5e2e32a8203f55b90e4d5170c97cfd64acddfd',
  'POST /\$procs/confirmEnrolment': '5827981f4d15fbcab970f2a7d4e8586cd702a57d2d6deda88bbe91b0c388c29f',
  'POST /\$procs/confirmRenewal': '3ca97573eb926a014607a4c785192f7671b4a17f664d1de86c0e507f59d5dc7e',
  'POST /\$procs/revokeDevice': 'f8901e52cad7ee1a959975b267134794345c01db223cbcf45fc361560fd59b98',
};

/// Hex of the whole-contract digest: moves when any op's contract does.
const String cratestackClientContractSha256 = '614df7f4bc03b30dcf8dcb35978e5bcec337f7e92da528a346ad75f25b835f04';

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
  'gwGQhmdhbWJpZW50A2VwaG9uZQBrdXNlclByZXNlbnQDhmdhbWJpZW50AGtlbnJvbG1lbnRJZABrdXNlclByZXNlbnQAiGNjcnYAY2t0eQBheABheQCCZGFyZ3MBhGhkZXZpY2VJZABkcm9sZQCEaWNoYWxsZW5nZQBrY2hhbGxlbmdlSWQAgmRhcmdzBYhnYW1iaWVudABrZW5yb2xtZW50SWQAY290cABrdXNlclByZXNlbnQAjmdhbWJpZW50AGhkZXZpY2VJZABlZW1haWwAZG5hbWUAZXBob25lAGZ1c2VySWQAa3VzZXJQcmVzZW50AIJkYXJncwiEa2NoYWxsZW5nZUlkAGlzaWduYXR1cmUAgmthdHRlc3RhdGlvbgCCZGFyZ3MLgmhkZXZpY2VJZACCaGRldmljZUlkAIJkYXJncw6FhHgbUE9TVCAvJHByb2NzL2JlZ2luRW5yb2xtZW50WCABcxfPrMXxZe9WtEduivW5OKtGGaTKagOp7ugY8eU5YQQChHgZUE9TVCAvJHByb2NzL2JlZ2luUmVuZXdhbFgg706Urss1QyprWhwzLF4uMqggP1W5Dk1RcMl8/WSs3f0HBoR4HVBPU1QgLyRwcm9jcy9jb25maXJtRW5yb2xtZW50WCBYJ5gfTRX7yrlw8qfU6Fhs1wKlfS1t7aiLvpGww4jCnwoJhHgbUE9TVCAvJHByb2NzL2NvbmZpcm1SZW5ld2FsWCA8qXVz65JqAUYHpMeFGS92cbShf2ZNHehsDlB/WdXcfg0MhHgZUE9TVCAvJHByb2NzL3Jldm9rZURldmljZVgg+JAeUsrX7hqVmXWyZxNHlDRcAdsiPLz0X8NhVg/Vm5gQDw==',
);

