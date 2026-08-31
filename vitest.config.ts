import { defineConfig } from "vitest/config";
import path from "node:path";

export default defineConfig({
  resolve: {
    alias: { $lib: path.resolve(import.meta.dirname, "src/lib") },
  },
  test: {
    environment: "jsdom",
    include: ["tests/**/*.test.ts"],
  },
});
