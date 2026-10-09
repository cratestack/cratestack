import { defineConfig } from "vitest/config";

// jsdom, because the hooks under test render through `@testing-library/react`.
export default defineConfig({
  test: {
    environment: "jsdom",
    globals: false,
  },
});
