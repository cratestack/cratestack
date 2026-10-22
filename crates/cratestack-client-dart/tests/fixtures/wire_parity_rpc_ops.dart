// RPC twin of `wire_parity_rest_ops.dart`: every operation of `ci_rpc.cstack`
// through a generated client, plus the three adapter entry points the
// generated API classes do not reach (`batch`, `stream`, and a unary call
// with an idempotency key). See that file for how it is used.
import 'package:dart_verify_ci_rpc/dart_verify_ci_rpc.dart';

Future<void> _ignore(Future<Object?> Function() call) async {
  try {
    await call();
  } catch (_) {
    // The canned `204` makes response decoding fail; the request was recorded.
  }
}

Future<void> runRpcOps(DartVerifyCiRpcCratestackClient client) async {
  const headers = CratestackRpcCallOptions(headers: {'x-auth-id': '1'});
  const keyed = CratestackRpcCallOptions(
    headers: {'x-auth-id': '1'},
    idempotencyKey: 'idem-1',
  );

  await _ignore(() => client.authors.list());
  await _ignore(
    () => client.authors.list(
      input: const {'where': 'name=a b', 'limit': 5},
      options: headers,
    ),
  );
  await _ignore(() => client.authors.get(1));
  await _ignore(
    () => client.authors.create(const CreateAuthorInput(id: 1, name: 'Ada')),
  );
  await _ignore(
    () => client.authors.update(1, const UpdateAuthorInput(name: 'Grace')),
  );
  await _ignore(() => client.authors.delete(1, options: keyed));

  await _ignore(() => client.posts.list());
  await _ignore(() => client.posts.get(7, options: headers));
  await _ignore(
    () => client.posts.create(
      CreatePostInput(
        id: 7,
        title: 'Hello, wörld',
        status: PostStatus.draft,
        published: true,
        authorId: 1,
        viewCount: BigInt.parse('9223372036854775807'),
      ),
      options: keyed,
    ),
  );
  await _ignore(
    () => client.posts.update(
      7,
      const UpdatePostInput(title: 'Renamed', published: false),
    ),
  );
  await _ignore(() => client.posts.delete(7));

  await _ignore(
    () => client.procedures.searchPosts(
      const SearchPostsArgs(
        filter: PostStatusFilter(statuses: [PostStatus.draft]),
      ),
    ),
  );
  await _ignore(
    () => client.procedures.currentStatus(
      const CurrentStatusArgs(postId: 7),
      options: keyed,
    ),
  );

  await _ignore(
    () => client.adapter.batch(
      const [
        RpcRequest(id: 1, op: 'model.Author.get', input: {'id': 1}),
        RpcRequest(
          id: 2,
          op: 'model.Post.list',
          input: {'limit': 5},
          idem: 'frame-idem',
        ),
      ],
      options: headers,
    ),
  );
  await _ignore(
    () => client.adapter
        .stream('model.Post.list', const {'limit': 2}, options: headers)
        .toList(),
  );
}
