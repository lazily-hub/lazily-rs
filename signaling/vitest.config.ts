import { defineWorkersConfig } from "@cloudflare/vitest-pool-workers/config";

export default defineWorkersConfig({
  test: {
    poolOptions: {
      workers: {
        // The SignalingRoom DO holds long-lived WebSockets and no storage API,
        // which is incompatible with per-test isolated-storage teardown. Run a
        // single shared worker with isolation off instead.
        isolatedStorage: false,
        singleWorker: true,
        wrangler: { configPath: "./wrangler.toml" },
      },
    },
  },
});
