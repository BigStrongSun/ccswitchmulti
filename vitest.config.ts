import path from "node:path";
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  test: {
    environment: "jsdom",
    setupFiles: ["./tests/setupGlobals.ts", "./tests/setupTests.ts"],
    globals: true,
    // Linked git worktrees under .worktrees/ carry their own src/ and
    // node_modules; exclude them so the main tree's test run stays isolated.
    exclude: ["**/.worktrees/**", "**/node_modules/**", "**/dist/**"],
    coverage: {
      reporter: ["text", "lcov"],
    },
  },
});
