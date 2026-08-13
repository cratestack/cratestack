import {
  useMutation,
  useQuery,
  type UseMutationOptions,
  type UseQueryOptions,
} from "@tanstack/react-query";
import type { ReactViteSwrClientClient } from "./client.js";
import type {
  FocusEstimateArgs,
  FocusEstimateResult,
  Board,
  CreateBoardInput,
  UpdateBoardInput,
  BoardWhere,
  BoardOrderByClause,
  BoardFindMany,
  Task,
  CreateTaskInput,
  UpdateTaskInput,
  TaskWhere,
  TaskOrderByClause,
  TaskFindMany,
  EstimateFocusMinutesArgs,
  BoardSortField,
  TaskSortField,
  Page,
} from "./models.js";
import type { CratestackQueryRequestConfig, CratestackRequestConfig } from "./queries.js";

export const cratestackQueryKeys = {
  boardList: (options?: CratestackQueryRequestConfig) => ["/boards", "list", options?.query] as const,
  boardDetail: (id: number, options?: CratestackQueryRequestConfig) => ["/boards", "detail", id, options?.query] as const,
  taskList: (options?: CratestackQueryRequestConfig) => ["/tasks", "list", options?.query] as const,
  taskDetail: (id: number, options?: CratestackQueryRequestConfig) => ["/tasks", "detail", id, options?.query] as const,
  estimateFocusMinutesProcedure: (args: EstimateFocusMinutesArgs) => ["/$procs/estimateFocusMinutes", args] as const,
};

export function useBoardListQuery(
  client: ReactViteSwrClientClient,
  options: CratestackQueryRequestConfig & {
    queryOptions?: Omit<UseQueryOptions<Board[]>, "queryKey" | "queryFn">;
  } = {},
) {
  return useQuery({
    ...options.queryOptions,
    queryKey: cratestackQueryKeys.boardList(options),
    queryFn: ({ signal }) => client.boards.list({ ...options, signal }),
  });
}

export function useBoardQuery(
  client: ReactViteSwrClientClient,
  id: number,
  options: CratestackQueryRequestConfig & {
    queryOptions?: Omit<UseQueryOptions<Board>, "queryKey" | "queryFn">;
  } = {},
) {
  return useQuery({
    ...options.queryOptions,
    queryKey: cratestackQueryKeys.boardDetail(id, options),
    queryFn: ({ signal }) => client.boards.get(id, { ...options, signal }),
  });
}

export function useCreateBoardMutation(
  client: ReactViteSwrClientClient,
  options?: UseMutationOptions<Board, Error, CreateBoardInput>,
) {
  return useMutation({
    ...options,
    mutationKey: ["boardCreate"],
    mutationFn: (input) => client.boards.create(input),
  });
}

export function useUpdateBoardMutation(
  client: ReactViteSwrClientClient,
  options?: UseMutationOptions<Board, Error, { id: number; input: UpdateBoardInput }>,
) {
  return useMutation({
    ...options,
    mutationKey: ["boardUpdate"],
    mutationFn: ({ id, input }) => client.boards.update(id, input),
  });
}

export function useDeleteBoardMutation(
  client: ReactViteSwrClientClient,
  options?: UseMutationOptions<void, Error, { id: number }>,
) {
  return useMutation({
    ...options,
    mutationKey: ["boardDelete"],
    mutationFn: ({ id }) => client.boards.delete(id),
  });
}

export function useTaskListQuery(
  client: ReactViteSwrClientClient,
  options: CratestackQueryRequestConfig & {
    queryOptions?: Omit<UseQueryOptions<Task[]>, "queryKey" | "queryFn">;
  } = {},
) {
  return useQuery({
    ...options.queryOptions,
    queryKey: cratestackQueryKeys.taskList(options),
    queryFn: ({ signal }) => client.tasks.list({ ...options, signal }),
  });
}

export function useTaskQuery(
  client: ReactViteSwrClientClient,
  id: number,
  options: CratestackQueryRequestConfig & {
    queryOptions?: Omit<UseQueryOptions<Task>, "queryKey" | "queryFn">;
  } = {},
) {
  return useQuery({
    ...options.queryOptions,
    queryKey: cratestackQueryKeys.taskDetail(id, options),
    queryFn: ({ signal }) => client.tasks.get(id, { ...options, signal }),
  });
}

export function useCreateTaskMutation(
  client: ReactViteSwrClientClient,
  options?: UseMutationOptions<Task, Error, CreateTaskInput>,
) {
  return useMutation({
    ...options,
    mutationKey: ["taskCreate"],
    mutationFn: (input) => client.tasks.create(input),
  });
}

export function useUpdateTaskMutation(
  client: ReactViteSwrClientClient,
  options?: UseMutationOptions<Task, Error, { id: number; input: UpdateTaskInput }>,
) {
  return useMutation({
    ...options,
    mutationKey: ["taskUpdate"],
    mutationFn: ({ id, input }) => client.tasks.update(id, input),
  });
}

export function useDeleteTaskMutation(
  client: ReactViteSwrClientClient,
  options?: UseMutationOptions<void, Error, { id: number }>,
) {
  return useMutation({
    ...options,
    mutationKey: ["taskDelete"],
    mutationFn: ({ id }) => client.tasks.delete(id),
  });
}

export function useEstimateFocusMinutesQuery(
  client: ReactViteSwrClientClient,
  args: EstimateFocusMinutesArgs,
  options?: Omit<UseQueryOptions<FocusEstimateResult>, "queryKey" | "queryFn">,
) {
  return useQuery({
    ...options,
    queryKey: cratestackQueryKeys.estimateFocusMinutesProcedure(args),
    queryFn: () => client.procedures.estimateFocusMinutes(args),
  });
}

