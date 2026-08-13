// Shared loading/error rendering for every resource page below — kept as
// a tiny component rather than duplicated three times.
export function QueryState({
  isLoading,
  isError,
  error,
}: {
  isLoading: boolean;
  isError: boolean;
  error: unknown;
}) {
  if (isLoading) return <p className="muted">Loading…</p>;
  if (isError) {
    const message = error instanceof Error ? error.message : String(error);
    return <p className="error">{message}</p>;
  }
  return null;
}
