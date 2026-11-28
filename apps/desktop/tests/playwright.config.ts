import { defineConfig, devices } from "@playwright/test";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const desktopDir = fileURLToPath(new URL("..", import.meta.url));

export default defineConfig({
  testDir: ".",
  testIgnore: "**/fixture.spec.ts",
  outputDir: path.join(os.tmpdir(), "slabbase-playwright"),
  fullyParallel: true,
  workers: process.env.CI ? 1 : undefined,
  timeout: 30_000,
  expect: { timeout: 6_000 },
  reporter: [["list"]],
  use: {
    baseURL: "http://127.0.0.1:5199",
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
      name: "fixture",
      testMatch: "**/fixture.spec.ts",
      testIgnore: [],
      fullyParallel: false,
    },
    {
      name: "820x840",
      use: { ...devices["Desktop Chrome"], viewport: { width: 820, height: 840 } },
    },
    {
      name: "1024x680",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1024, height: 680 } },
    },
    {
      name: "1440x900",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } },
    },
    {
      name: "2560x1440",
      use: { ...devices["Desktop Chrome"], viewport: { width: 2560, height: 1440 } },
    },
    {
      name: "3440x1440",
      use: { ...devices["Desktop Chrome"], viewport: { width: 3440, height: 1440 } },
    },
    {
      name: "5120x1440",
      grep: /@superwide/,
      use: { ...devices["Desktop Chrome"], viewport: { width: 5120, height: 1440 } },
    },
    {
      name: "1440x900-hidpi",
      grep: /@hidpi/,
      use: {
        ...devices["Desktop Chrome"],
        viewport: { width: 1440, height: 900 },
        deviceScaleFactor: 2,
      },
    },
  ],
});
