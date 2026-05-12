# cratestack-client-flutter

Flutter integration for CrateStack clients.

## Overview

`cratestack-client-flutter` provides Flutter-specific widgets and state management integration for CrateStack clients, including offline support and FFI bridging.

## Installation

```toml
[dependencies]
cratestack-client-flutter = "0.2"
```

## Note

This crate is designed for Flutter-over-FFI architectures where Rust handles state and business logic while Flutter is UI-only. See [Offline-First with SQLite](https://cratestack.dev/guides/offline-first-sqlite) for the architecture.

## Widget Integration

```dart
import 'package:cratestack_client_flutter/cratestack_client_flutter.dart';

class UserListWidget extends StatefulWidget {
  @override
  _UserListWidgetState createState() => _UserListWidgetState();
}

class _UserListWidgetState extends State<UserListWidget> {
  final client = CratestackClient(
    baseUrl: 'https://api.example.com',
    codec: CborCodec(),
  );

  @override
  Widget build(BuildContext context) {
    return FutureBuilder<List<User>>(
      future: client.users.list(limit: 10),
      builder: (context, snapshot) {
        if (snapshot.hasData) {
          return ListView(
            children: snapshot.data!
                .map((user) => ListTile(title: Text(user.email)))
                .toList(),
          );
        }
        if (snapshot.hasError) {
          return Text('Error: ${snapshot.error}');
        }
        return CircularProgressIndicator();
      },
    );
  }
}
```

## FFI Integration

For on-device SQLite, use `cratestack-rusqlite` with a sync API:

```rust
// Rust side (FFI)
use cratestack::RusqliteRuntime;
use cratestack_rusqlite::ModelDelegate;

#[no_mangle]
pub extern "C" fn ffi_create_user(runtime: *mut RusqliteRuntime, input: *const c_char) -> *mut c_char {
    // Sync API, no tokio
    let notes = ModelDelegate::new(runtime, &NOTE_MODEL);
    let created = notes.create(input).run()?;
    json_response_into(&created)
}
```

## See Also

- [Offline-First with SQLite](https://cratestack.dev/guides/offline-first-sqlite)
- `cratestack-rusqlite` - Sync SQLite backend
- `cratestack-client-dart` - Generated Dart client

## License

MIT