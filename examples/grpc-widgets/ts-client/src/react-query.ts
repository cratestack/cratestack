import {
  useMutation,
  useQuery,
  type UseMutationOptions,
  type UseQueryOptions,
} from "@tanstack/react-query";
import type { CratestackExamplesWidgetsGrpcClientClient, GrpcListInput } from "./client";
import type {
  Widget,
  CreateWidgetInput,
  UpdateWidgetInput,
  Page,
} from "./models";
import type { CratestackGrpcCallOptions } from "./runtime";

// No procedure hooks here: `transport grpc` schemas never route
// procedures over gRPC-Web (ticket #171 didn't wire them into the
// generated tonic service — see `crate::grpc`'s module doc), so
// `procedures`/`query_procedures`/`mutation_procedures` are always empty
// for this transport.

export const cratestackQueryKeys = {
  widgetList: (input?: GrpcListInput) => ["model.Widget.list", input] as const,
  widgetDetail: (id: number) => ["model.Widget.get", id] as const,
};

export function useWidgetListQuery(
  client: CratestackExamplesWidgetsGrpcClientClient,
  input: GrpcListInput = {},
  options: CratestackGrpcCallOptions & {
    queryOptions?: Omit<UseQueryOptions<Page<Widget>>, "queryKey" | "queryFn">;
  } = {},
) {
  return useQuery({
    ...options.queryOptions,
    queryKey: cratestackQueryKeys.widgetList(input),
    queryFn: ({ signal }) => client.widgets.list(input, { ...options, signal }),
  });
}

export function useWidgetQuery(
  client: CratestackExamplesWidgetsGrpcClientClient,
  id: number,
  options: CratestackGrpcCallOptions & {
    queryOptions?: Omit<UseQueryOptions<Widget>, "queryKey" | "queryFn">;
  } = {},
) {
  return useQuery({
    ...options.queryOptions,
    queryKey: cratestackQueryKeys.widgetDetail(id),
    queryFn: ({ signal }) => client.widgets.get(id, { ...options, signal }),
  });
}

export function useCreateWidgetMutation(
  client: CratestackExamplesWidgetsGrpcClientClient,
  options?: UseMutationOptions<Widget, Error, CreateWidgetInput>,
) {
  return useMutation({
    ...options,
    mutationKey: ["createWidget"],
    mutationFn: (input) => client.widgets.create(input),
  });
}

export function useUpdateWidgetMutation(
  client: CratestackExamplesWidgetsGrpcClientClient,
  options?: UseMutationOptions<Widget, Error, { id: number; input: UpdateWidgetInput }>,
) {
  return useMutation({
    ...options,
    mutationKey: ["updateWidget"],
    mutationFn: ({ id, input }) => client.widgets.update(id, input),
  });
}

export function useDeleteWidgetMutation(
  client: CratestackExamplesWidgetsGrpcClientClient,
  options?: UseMutationOptions<void, Error, { id: number }>,
) {
  return useMutation({
    ...options,
    mutationKey: ["deleteWidget"],
    mutationFn: ({ id }) => client.widgets.delete(id),
  });
}

