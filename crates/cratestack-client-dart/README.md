# cratestack-client-dart

Dart/Flutter client generator for CrateStack services.

## Overview

`cratestack-client-dart` generates a complete Dart package from a `.cstack` schema, including typed models, client API, and optional Flutter integration.

## Installation

This is a build-time dependency used by the CLI:

```bash
cratestack generate-dart --schema schema.cstack --out ./dart_client --name my_api_client
```

## Generated Package Structure

```
my_api_client/
├── pubspec.yaml
├── lib/
│   ├── my_api_client.dart
│   └── src/
│       ├── runtime.dart      # HTTP client, codec support
│       ├── models.dart       # Generated model classes
│       ├── client.dart       # API client
│       └── queries.dart      # Selection builders
├── test/
│   └── client_test.dart
└── analysis_options.yaml
```

## Generated Models

```dart
class User {
  final String id;
  final String email;
  final String? name;
  final DateTime createdAt;

  User({
    required this.id,
    required this.email,
    this.name,
    required this.createdAt,
  });

  factory User.fromJson(Map<String, dynamic> json) => /* ... */;
  Map<String, dynamic> toJson() => /* ... */;
}

class CreateUserInput {
  final String email;
  final String? name;

  CreateUserInput({required this.email, this.name});
  Map<String, dynamic> toJson() => /* ... */;
}
```

## Client Usage

```dart
import 'package:my_api_client/my_api_client.dart';

void main() async {
  final client = CratestackClient(
    baseUrl: 'https://api.example.com',
    codec: CborCodec(),
  );

  final api = MyApiClient(client);

  // List
  final users = await api.users.list(limit: 10);

  // Get with projection
  final user = await api.users.getView(
    id: 'usr_123',
    selection: UserSelect()
      ..id = true
      ..email = true
      ..includePosts = (PostInclude()..id = true ..title = true),
  );

  // Create
  final created = await api.users.create(CreateUserInput(
    email: 'user@example.com',
    name: 'Alice',
  ));
}
```

## Flutter Integration

For Flutter apps, use with `cratestack-client-flutter` for state persistence and offline support.

## Codecs

```dart
// CBOR (recommended)
final codec = CborCodec();

// JSON (development/debugging)
final codec = JsonCodec();
```

## See Also

- `cratestack-client-flutter` - Flutter-specific integrations
- `cratestack-cli` - CLI for code generation

## License

MIT