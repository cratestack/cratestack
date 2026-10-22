// Drives every REST operation of `ci_rest.cstack` (list / get / create /
// update / delete on both models, plus both procedures) through a generated
// client, with and without query parameters and per-call headers.
//
// Shared by `wire_parity_rest_test.dart` (which replays it through the HTTP
// adapter and compares the recorded requests with `wire-golden/rest-*.json`)
// and by the one-off harness that captured those golden files from the
// pre-seam `*DioAdapter` classes (cratestack#1204). Results are ignored: the
// recording transport answers `204`, so most response decoders throw, and the
// request is all that matters here.
import 'package:dart_verify_ci_rest/dart_verify_ci_rest.dart';

Future<void> _ignore(Future<Object?> Function() call) async {
  try {
    await call();
  } catch (_) {
    // The canned `204` makes response decoding fail; the request was recorded.
  }
}

Future<void> runRestOps(DartVerifyCiRestCratestackClient client) async {
  const listQuery = CratestackListQuery(
    limit: 20,
    offset: 40,
    sort: '-id',
    where: 'published=true',
    orFilters: ['title=a b', 'title=ü/é +&='],
    filters: {'status': 'draft', 'q': 'x y&z'},
    fields: ['id', 'title'],
    include: ['author'],
    includeFields: {
      'author': ['id', 'name'],
    },
  );
  const fetchQuery = CratestackFetchQuery(
    fields: ['id', 'name'],
    include: ['posts'],
    includeFields: {
      'posts': ['id'],
    },
  );
  const headers = CratestackCallOptions(headers: {'x-auth-id': '1'});

  await _ignore(() => client.authors.list());
  await _ignore(() => client.authors.list(query: listQuery, options: headers));
  await _ignore(() => client.authors.get(1));
  await _ignore(() => client.authors.get(2, query: fetchQuery));
  await _ignore(
    () => client.authors.create(const CreateAuthorInput(id: 1, name: 'Ada')),
  );
  await _ignore(
    () => client.authors.update(1, const UpdateAuthorInput(name: 'Grace')),
  );
  await _ignore(() => client.authors.delete(1, options: headers));

  await _ignore(() => client.posts.list());
  await _ignore(() => client.posts.list(query: listQuery, options: headers));
  await _ignore(() => client.posts.get(7));
  await _ignore(() => client.posts.get(8, query: fetchQuery));
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
      options: headers,
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
        filter: PostStatusFilter(statuses: [PostStatus.draft, PostStatus.published]),
      ),
    ),
  );
  await _ignore(() => client.procedures.searchPosts(const SearchPostsArgs()));
  await _ignore(
    () => client.procedures.currentStatus(
      const CurrentStatusArgs(postId: 7),
      options: headers,
    ),
  );
}
