import {
  useMutation,
  useQuery,
  type UseMutationOptions,
  type UseQueryOptions,
} from "@tanstack/react-query";
import type { TinyRpcClientClient } from "./client";
import type {
  Widget,
  CreateWidgetInput,
  UpdateWidgetInput,
  EchoNameArgs,
  Page,
} from "./models";
import type { CratestackRpcCallOptions } from "./runtime";

export const cratestackQueryKeys = {
  widgetList: (input?: Record<string, unknown>) => ["model.Widget.list", input] as const,
  widgetDetail: (id: number) => ["model.Widget.get", id] as const,
  echoNameProcedure: (args: EchoNameArgs) => ["procedure.echoName", args] as const,
};

export function useWidgetListQuery(
  client: TinyRpcClientClient,
  input: Record<string, unknown> = {},
  options: CratestackRpcCallOptions & {
    queryOptions?: Omit<UseQueryOptions<Widget[]>, "queryKey" | "queryFn">;
  } = {},
) {
  return useQuery({
    ...options.queryOptions,
    queryKey: cratestackQueryKeys.widgetList(input),
    queryFn: ({ signal }) => client.widgets.list(input, { ...options, signal }),
  });
}

export function useWidgetQuery(
  client: TinyRpcClientClient,
  id: number,
  options: CratestackRpcCallOptions & {
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
  client: TinyRpcClientClient,
  options?: UseMutationOptions<Widget, Error, CreateWidgetInput>,
) {
  return useMutation({
    ...options,
    mutationKey: ["widgetCreate"],
    mutationFn: (input) => client.widgets.create(input),
  });
}

export function useUpdateWidgetMutation(
  client: TinyRpcClientClient,
  options?: UseMutationOptions<Widget, Error, { id: number; input: UpdateWidgetInput }>,
) {
  return useMutation({
    ...options,
    mutationKey: ["widgetUpdate"],
    mutationFn: ({ id, input }) => client.widgets.update(id, input),
  });
}

export function useDeleteWidgetMutation(
  client: TinyRpcClientClient,
  options?: UseMutationOptions<void, Error, { id: number }>,
) {
  return useMutation({
    ...options,
    mutationKey: ["widgetDelete"],
    mutationFn: ({ id }) => client.widgets.delete(id),
  });
}

export function useEchoNameQuery(
  client: TinyRpcClientClient,
  args: EchoNameArgs,
  options?: Omit<UseQueryOptions<string>, "queryKey" | "queryFn">,
) {
  return useQuery({
    ...options,
    queryKey: cratestackQueryKeys.echoNameProcedure(args),
    queryFn: () => client.procedures.echoName(args),
  });
}

