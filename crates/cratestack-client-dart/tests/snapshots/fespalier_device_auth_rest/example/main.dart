import 'package:device_auth_rest_client/device_auth_rest_client.dart';

void main() {
  const fetchQuery = CratestackFetchQuery();
  const listQuery = CratestackListQuery(
    sort: '-id',
    limit: 20,
    offset: 0,
    where: 'published=true',
  );

  // Generated model API entry points:

  // Generated procedures:
  // - beginEnrolment(...)
  // - confirmEnrolment(...)
  // - beginRenewal(...)
  // - confirmRenewal(...)
  // - revokeDevice(...)

  assert(listQuery.limit == 20);
  assert(fetchQuery.toQueryParameters().isEmpty);
}
