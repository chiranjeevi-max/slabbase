import { defineConfig, devices } from "@playwright/test";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const desktopDir = fileURLToPath(new URL("..", import.meta.url));

export default defineConfig({
  testDir: ".",
  outputDir: path.join(os.tmpdir(), "slabbase-playwright"),
  fullyParallel: true,
  workers: 2,
  timeout: 30_000,
  expect: { timeout: 6_000 },
  reporter: [["list"]],
  use: {
    baseURL: "http://127.0.0.1:5199",
    channel: "chrome",
    headless: true,
    viewport: { width: 1280, height: 800 },
    colorScheme: "light",
    reducedMotion: "reduce",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "npm run dev -- --port 5199",
    url: "http://127.0.0.1:5199",
    cwd: desktopDir,
    reuseExistingServer: false,
    timeout: 60_000,
  },
  projects: [
    {
      name: "1024x680",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1024, height: 680 } },
    },
    {
      name: "1280x800",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } },
    },
    {
      name: "1440x900",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } },
    },
  ],
});
