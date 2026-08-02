import type { RpcLink } from "@cratestack/ts-types";

/** Convenience copy of the reference link generated into every
 *  `transport rpc` project's `src/links.ts` (issue #182). Not a
 *  re-export — there's no shared import path between this package and
 *  per-project generated code — kept in sync by this package's own
 *  test coverage instead. Logs each call's kind, op id, outcome, and
 *  duration; never touches `response.body`. */
export function createLoggerLink(logger: Pick<Console, "info" | "error"> = console): RpcLink {
  return async (request, next) => {
    const start = Date.now();
    logger.info(`[rpc] -> ${request.kind} ${request.opId}`);
    try {
      const result = await next(request);
      logger.info(`[rpc] <- ${request.opId} ${result.response.status} (${Date.now() - start}ms)`);
      return result;
    } catch (error) {
      logger.error(`[rpc] x ${request.opId} failed (${Date.now() - start}ms)`, error);
      throw error;
    }
  };
}
