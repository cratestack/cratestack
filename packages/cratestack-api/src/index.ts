// @cratestack/api is now a thin compat re-export over the split
// @cratestack/{ts-types,link-*,runtime-*,validator-*,adapter-*} family —
// see README.md. The root entry point only re-exports the three pieces
// this package originally shipped (no new peer dependency is required
// just to `import from "@cratestack/api"`); everything net-new is a
// named subpath import instead, so e.g. zod/yup/axios/tanstack-query/
// rtk never become implicit peer requirements of the root import.
export * from "@cratestack/ts-types";
export * from "@cratestack/link-batch";
export * from "@cratestack/link-logger";
