import type { JsonValue } from "./runtime";

export interface PageInfo {
  limit?: number;
  offset?: number;
  hasNext?: boolean;
  nextOffset?: number | null;
  total?: number | null;
}

export interface Page<T> {
  items: T[];
  pageInfo?: PageInfo;
}

export interface Widget {
  id?: number;
  name?: string;
  weight?: number | null;
}

export interface CreateWidgetInput {
  id: number;
  name: string;
  weight?: number | null;
}

export interface UpdateWidgetInput {
  name?: string;
  weight?: number | null;
}

export interface EchoNameArgs {
  name: string;
}

