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
    // Linked worktrees and local task snapshots carry copied test trees. They
    // are evidence/artifacts, not additional suites for the active checkout.
    exclude: [
      "**/.worktrees/**",
      "**/.tmp/**",
      "**/node_modules/**",
      "**/dist/**",
    ],
    coverage: {
      reporter: ["text", "lcov"],
    },
  },
});
