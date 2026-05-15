import {
  useMutation,
  useQuery,
  type UseMutationOptions,
  type UseQueryOptions,
} from "@tanstack/react-query";
import type { TinyRestClientClient } from "./client";
import type {
  Widget,
  CreateWidgetInput,
  UpdateWidgetInput,
  EchoNameArgs,
  Page,
} from "./models";
import type { CratestackQueryRequestConfig, CratestackRequestConfig } from "./queries";

export const cratestackQueryKeys = {
  widgetList: (options?: CratestackQueryRequestConfig) => ["/widgets", "list", options?.query] as const,
  widgetDetail: (id: number, options?: CratestackQueryRequestConfig) => ["/widgets", "detail", id, options?.query] as const,
  echoNameProcedure: (args: EchoNameArgs) => ["/$procs/echoName", args] as const,
};

export function useWidgetListQuery(
  client: TinyRestClientClient,
  options: CratestackQueryRequestConfig & {
    queryOptions?: Omit<UseQueryOptions<Widget[]>, "queryKey" | "queryFn">;
  } = {},
) {
  return useQuery({
    ...options.queryOptions,
    queryKey: cratestackQueryKeys.widgetList(options),
    queryFn: ({ signal }) => client.widgets.list({ ...options, signal }),
  });
}

export function useWidgetQuery(
  client: TinyRestClientClient,
  id: number,
  options: CratestackQueryRequestConfig & {
    queryOptions?: Omit<UseQueryOptions<Widget>, "queryKey" | "queryFn">;
  } = {},
) {
  return useQuery({
    ...options.queryOptions,
    queryKey: cratestackQueryKeys.widgetDetail(id, options),
    queryFn: ({ signal }) => client.widgets.get(id, { ...options, signal }),
  });
}

export function useCreateWidgetMutation(
  client: TinyRestClientClient,
  options?: UseMutationOptions<Widget, Error, CreateWidgetInput>,
) {
  return useMutation({
    ...options,
    mutationKey: ["widgetCreate"],
    mutationFn: (input) => client.widgets.create(input),
  });
}

export function useUpdateWidgetMutation(
  client: TinyRestClientClient,
  options?: UseMutationOptions<Widget, Error, { id: number; input: UpdateWidgetInput }>,
) {
  return useMutation({
    ...options,
    mutationKey: ["widgetUpdate"],
    mutationFn: ({ id, input }) => client.widgets.update(id, input),
  });
}

export function useDeleteWidgetMutation(
  client: TinyRestClientClient,
  options?: UseMutationOptions<void, Error, { id: number }>,
) {
  return useMutation({
    ...options,
    mutationKey: ["widgetDelete"],
    mutationFn: ({ id }) => client.widgets.delete(id),
  });
}

export function useEchoNameQuery(
  client: TinyRestClientClient,
  args: EchoNameArgs,
  options?: Omit<UseQueryOptions<string>, "queryKey" | "queryFn">,
) {
  return useQuery({
    ...options,
    queryKey: cratestackQueryKeys.echoNameProcedure(args),
    queryFn: () => client.procedures.echoName(args),
  });
}

