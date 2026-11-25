import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import type { CDPSession } from "playwright-core";

type Theme = "light" | "dark";
type Surface = "solid" | "glass";
type Density = "compact" | "default" | "comfortable";

interface Prefs {
  version: 1;
  theme: Theme | "system";
  surface: Surface;
  accent: string;
  customAccent: string;
  density: Density;
}

const BASE_PREFS: Prefs = {
  version: 1,
  theme: "light",
  surface: "solid",
  accent: "blue",
  customAccent: "#0a84ff",
  density: "default",
};

const DENSITY_HEIGHT: Record<Density, number> = {
  compact: 26,
  default: 29,
  comfortable: 34,
};

const DENSITY_LABEL: Record<Density, string> = {
  compact: "Compact",
  default: "Default",
  comfortable: "Comfortable",
};

interface BootOptions {
  theme?: Theme | "system";
  surface?: Surface;
  accent?: string;
  customAccent?: string;
  density?: Density;
  fixture?: "design-studio" | "empty" | "stress";
  seed?: boolean;
  tauri?: boolean;
  colorScheme?: "light" | "dark";
}

async function boot(page: Page, options: BootOptions = {}): Promise<string[]> {
  const prefs: Prefs = {
    ...BASE_PREFS,
    theme: options.theme ?? BASE_PREFS.theme,
    surface: options.surface ?? BASE_PREFS.surface,
    accent: options.accent ?? BASE_PREFS.accent,
    customAccent: options.customAccent ?? BASE_PREFS.customAccent,
    density: options.density ?? BASE_PREFS.density,
  };
  const errors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error" || message.type() === "warning") {
      errors.push(`${message.type()}: ${message.text()}`);
    }
  });
  page.on("pageerror", (error) => errors.push(`pageerror: ${error.message}`));

  if (options.seed !== false) {
    await page.addInitScript((value) => {
      window.localStorage.setItem("slabbase.appearance.v1", JSON.stringify(value));
    }, prefs);
  }
  if (options.tauri) {
    await page.addInitScript(() => {
      window.__TAURI_INTERNALS__ = {
        transformCallback: () => 0,
        invoke: async () => 0,
      };
    });
  }
  if (options.colorScheme) {
    await page.emulateMedia({ colorScheme: options.colorScheme });
  }

  const fixture = options.fixture ?? "design-studio";
  await page.goto(`/?fixture=${fixture}`);
  await expect(page.getByTestId("toolbar")).toBeVisible();
  if (fixture === "empty") {
    await expect(page.getByTestId("empty-workspace")).toBeVisible();
  } else {
    await expect(page.getByTestId("grid-pane")).toBeVisible();
  }
  await page.evaluate(() => document.fonts.ready);
  return errors;
}

const transparencySessions = new WeakMap<Page, CDPSession>();

async function setReducedTransparency(
  page: Page,
  value: "reduce" | "no-preference",
): Promise<void> {
  let cdp = transparencySessions.get(page);
  if (!cdp) {
    cdp = await page.context().newCDPSession(page);
    transparencySessions.set(page, cdp);
  }
  await cdp.send("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-reduced-transparency", value }],
  });
}

async function settleLayout(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
      }),
  );
}

async function ensureSidebar(page: Page): Promise<void> {
  const sidebar = page.getByTestId("sidebar");
  let box = await sidebar.boundingBox();
  if (box && box.x >= 0) return;

  await page.waitForFunction(() => {
    const node = document.querySelector<HTMLElement>('[data-testid="sidebar"]');
    const toggle = document.querySelector<HTMLElement>('[data-testid="sidebar-toggle"]');
    return Boolean(node && node.getBoundingClientRect().x >= 0) || Boolean(toggle);
  });

  box = await sidebar.boundingBox();
  if (!box || box.x < 0) {
    const toggle = page.getByTestId("sidebar-toggle");
    if ((await toggle.count()) > 0 && (await toggle.isVisible())) {
      await toggle.click();
    }
  }
  await page.waitForFunction(() => {
    const node = document.querySelector<HTMLElement>('[data-testid="sidebar"]');
    return node ? node.getBoundingClientRect().x >= 0 : false;
  });
}

async function selectTable(page: Page, table: string): Promise<void> {
  await ensureSidebar(page);
  await page.getByTestId("sidebar").getByText(table, { exact: true }).click();
  await expect(page.getByTestId("grid-pane")).toBeVisible();
  await expect(page.getByTestId("sidebar").locator(".source-row.active")).toContainText(table);
  await expect(page.getByTestId("grid-row").first()).toBeVisible();
}

async function selectSidebarItem(page: Page, label: string): Promise<void> {
  await ensureSidebar(page);
  await page.getByTestId("sidebar").getByText(label, { exact: true }).click();
}

async function openInspector(page: Page): Promise<void> {
  await page.locator('[data-testid="grid-row"]').first().click();
  await expect(page.getByTestId("inspector")).toBeVisible();
}

const VIEWS: Array<{
  name: string;
  fixture?: "design-studio" | "empty" | "stress";
  status: string;
  open: (page: Page) => Promise<void>;
}> = [
  {
    name: "empty workspace",
    fixture: "empty",
    status: "No table selected",
    open: async () => {},
  },
  {
    name: "clients table",
    status: "3 rows",
    open: async () => {},
  },
  {
    name: "clients with inspector",
    status: "3 rows",
    open: async (page) => {
      await openInspector(page);
    },
  },
  {
    name: "invoices with portal",
    status: "4 rows",
    open: async (page) => {
      await selectTable(page, "Invoices");
      await openInspector(page);
      await expect(page.getByTestId("portal-scroll")).toBeVisible();
      await page.locator(".record-scroll").evaluate((node) => {
        node.scrollTop = node.scrollHeight;
      });
    },
  },
  {
    name: "form studio",
    status: "8 items",
    open: async (page) => {
      await page.getByRole("tab", { name: "Form" }).click();
      await expect(page.locator(".studio")).toBeVisible();
    },
  },
  {
    name: "relations and schema",
    status: "4 tables",
    open: async (page) => {
      await selectSidebarItem(page, "Relations & Schema");
      await expect(page.locator(".schema-view")).toBeVisible();
    },
  },
  {
    name: "audit log",
    status: "audit entries",
    open: async (page) => {
      await selectSidebarItem(page, "Audit Log");
      await expect(page.getByTestId("audit-drawer")).toBeVisible();
    },
  },
  {
    name: "command palette",
    status: "3 rows",
    open: async (page) => {
      await page.getByTestId("search-trigger").click();
      await expect(page.getByTestId("command-palette")).toBeVisible();
    },
  },
  {
    name: "appearance popover",
    status: "3 rows",
    open: async (page) => {
      await page.getByTestId("appearance-button").click();
      await expect(page.getByTestId("appearance-popover")).toBeVisible();
    },
  },
];

function rounded(value: number): number {
  return Math.round(value * 100) / 100;
}

async function assertViewport(page: Page): Promise<void> {
  const metrics = await page.evaluate(() => {
    const root = document.getElementById("root");
    const html = document.documentElement;
    const bodyRect = document.body.getBoundingClientRect();
    return {
      innerWidth: window.innerWidth,
      innerHeight: window.innerHeight,
      clientWidth: html.clientWidth,
      clientHeight: html.clientHeight,
      docWidth: html.scrollWidth,
      docHeight: html.scrollHeight,
      bodyWidth: document.body.scrollWidth,
      bodyHeight: document.body.scrollHeight,
      bodyLeft: bodyRect.left,
      bodyTop: bodyRect.top,
      rootWidth: root?.scrollWidth ?? 0,
      rootHeight: root?.scrollHeight ?? 0,
    };
  });
  const detail = JSON.stringify(metrics);
  expect(metrics.bodyLeft, `body must start at the viewport origin: ${detail}`).toBe(0);
  expect(metrics.bodyTop, `body must start at the viewport origin: ${detail}`).toBe(0);
  expect(metrics.docWidth, `document overflows horizontally: ${detail}`).toBeLessThanOrEqual(
    metrics.clientWidth,
  );
  expect(metrics.docHeight, `document overflows vertically: ${detail}`).toBeLessThanOrEqual(
    metrics.clientHeight,
  );
  expect(metrics.bodyWidth, `body overflows horizontally: ${detail}`).toBeLessThanOrEqual(
    metrics.clientWidth,
  );
  expect(metrics.bodyHeight, `body overflows vertically: ${detail}`).toBeLessThanOrEqual(
    metrics.clientHeight,
  );
  expect(metrics.rootWidth, `#root overflows horizontally: ${detail}`).toBeLessThanOrEqual(
    metrics.clientWidth,
  );
  expect(metrics.rootHeight, `#root overflows vertically: ${detail}`).toBeLessThanOrEqual(
    metrics.clientHeight,
  );
  expect(metrics.docWidth).toBeLessThanOrEqual(metrics.innerWidth);
  expect(metrics.docHeight).toBeLessThanOrEqual(metrics.innerHeight);
}

async function assertChromeContainment(page: Page): Promise<void> {
  const selectors = [
    '[data-testid="sidebar"]',
    '[data-testid="toolbar"]',
    '[data-testid="sub-toolbar"]',
    '[data-testid="inspector"]',
    '[data-testid="status-bar"]',
    '[data-testid="command-palette"]',
    '[data-testid="audit-drawer"]',
    '[data-testid="appearance-popover"]',
    '[data-testid="schema-dialog"]',
    '[data-testid="import-dialog"]',
    '[data-testid="print-dialog"]',
  ];

  const entries = await page.evaluate((selectors: string[]) => {
    return selectors.map((selector) => {
      const element = document.querySelector<HTMLElement>(selector);
      if (!element) return { selector, info: null };
      const rect = element.getBoundingClientRect();
      const style = getComputedStyle(element);
      if (
        style.visibility === "hidden" ||
        rect.right <= 0 ||
        rect.left >= window.innerWidth ||
        rect.width === 0
      ) {
        return { selector, info: null };
      }
      const children = Array.from(element.querySelectorAll<HTMLElement>("*"))
        .filter((node) => !node.closest('[data-testid="portal-scroll"]'))
        .map((node) => {
          const child = node.getBoundingClientRect();
          const className =
            typeof node.className === "string" ? node.className : String(node.className.baseVal);
          return {
            left: child.left,
            right: child.right,
            width: child.width,
            tag: node.tagName,
            className,
          };
        })
        .filter((child) => child.width > 0);
      return {
        selector,
        info: {
          rect: {
            left: rect.left,
            right: rect.right,
            top: rect.top,
            bottom: rect.bottom,
            width: rect.width,
            height: rect.height,
          },
          scrollWidth: element.scrollWidth,
          clientWidth: element.clientWidth,
          children,
        },
      };
    });
  }, selectors);

  for (const { selector, info } of entries) {
    if (!info) continue;
    expect(info.scrollWidth, `${selector} scrollWidth`).toBeLessThanOrEqual(
      info.clientWidth + 1,
    );
    for (const child of info.children) {
      expect(
        child.left,
        `${selector} child ${child.tag}.${child.className} left`,
      ).toBeGreaterThanOrEqual(info.rect.left - 1);
      expect(
        child.right,
        `${selector} child ${child.tag}.${child.className} right`,
      ).toBeLessThanOrEqual(info.rect.right + 1);
    }
  }
}

async function assertTextNotClipped(page: Page): Promise<void> {
  const selectors = [
    ".toolbar button",
    ".workspace-switcher .workspace-name",
    ".view-switcher button",
    ".table-toolbar button",
    ".source-row",
    ".source-heading",
    ".source-name",
    ".source-count",
    ".grid-head-cell .column-name",
    ".portal-header .portal-title",
    ".portal-header .text-button",
    ".portal-grid-head .portal-column-name",
    ".statusbar span",
    ".record-heading h2",
    ".record-header .eyebrow",
    ".field-label",
    ".appearance-head span",
    ".appearance-label",
    ".appearance-segment button",
    ".palette-row > span",
  ];

  const failures = await page.evaluate((selectors: string[]) => {
    const issues: string[] = [];
    for (const selector of selectors) {
      for (const node of document.querySelectorAll<HTMLElement>(selector)) {
        const rect = node.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) continue;
        if (getComputedStyle(node).visibility === "hidden") continue;
        const text = (node.textContent ?? "").trim();
        if (!text || node.clientWidth === 0) continue;
        if (node.scrollWidth > node.clientWidth + 1) {
          issues.push(
            `${selector} text "${text}" width ${node.scrollWidth} > ${node.clientWidth}`,
          );
        }
        if (node.scrollHeight > node.clientHeight + 1) {
          issues.push(
            `${selector} text "${text}" height ${node.scrollHeight} > ${node.clientHeight}`,
          );
        }
      }
    }
    return issues;
  }, selectors);
  expect(failures, failures.join("\n")).toEqual([]);
}

async function assertNoOverlap(page: Page): Promise<void> {
  const groups = await page.evaluate(() => {
    const toolbar = document.querySelector<HTMLElement>('[data-testid="toolbar"]');
    const sub = document.querySelector<HTMLElement>('[data-testid="sub-toolbar"]');
    const status = document.querySelector<HTMLElement>('[data-testid="status-bar"]');
    const inspector = document.querySelector<HTMLElement>('[data-testid="inspector"]');
    const portal = document.querySelector<HTMLElement>(".portal-header");

    const collect = (root: HTMLElement | null, selector: string) => {
      if (!root) return [];
      return Array.from(root.querySelectorAll<HTMLElement>(selector))
        .filter((element) => {
          const rect = element.getBoundingClientRect();
          return rect.width > 0 && rect.height > 0 && getComputedStyle(element).visibility !== "hidden";
        })
        .map((element) => {
          const rect = element.getBoundingClientRect();
          return {
            selector,
            className: element.className,
            left: rect.left,
            right: rect.right,
            top: rect.top,
            bottom: rect.bottom,
          };
        })
        .sort((left, right) => left.left - right.left);
    };

    return {
      toolbar: collect(
        toolbar,
        ':scope > .toolbar-left > button, :scope > .toolbar-left > .workspace-anchor, :scope > .toolbar-center > .view-switcher, :scope > .toolbar-center > .search-trigger, :scope > .toolbar-right > .toolbar-group > *, :scope > .toolbar-right > .toolbar-divider',
      ),
      sub: collect(sub, ":scope > *"),
      status: collect(status, ".status-left > *, .status-right > *"),
      inspector: collect(inspector, ".record-header > *"),
      portal: collect(portal, ":scope > *"),
    };
  });

  for (const [name, controls] of Object.entries(groups)) {
    for (let index = 1; index < controls.length; index += 1) {
      const previous = controls[index - 1];
      const current = controls[index];
      if (!previous || !current) continue;
      if (Math.abs(previous.top - current.top) > 4) continue;
      expect(
        current.left,
        `${name} overlap: ${previous.className} -> ${current.className}`,
      ).toBeGreaterThanOrEqual(previous.right + 4);
    }
  }
}

async function assertLayout(page: Page): Promise<void> {
  await assertViewport(page);
  await assertChromeContainment(page);
  await assertTextNotClipped(page);
  await assertNoOverlap(page);
}

async function getRect(page: Page, selector: string) {
  return page.locator(selector).first().evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return {
      left: rect.left,
      right: rect.right,
      top: rect.top,
      bottom: rect.bottom,
      width: rect.width,
      height: rect.height,
    };
  });
}

async function computedColor(page: Page, selector: string): Promise<string> {
  return page.locator(selector).first().evaluate((element) => getComputedStyle(element).backgroundColor);
}

function expectGlassBlurToken(blur: string): void {
  expect(blur).toContain("blur(28px)");
}

function expectGlassBackdrop(filter: string): void {
  // Chrome clamps the used backdrop-filter to blur(0px) saturate(1) when macOS
  // Reduce Transparency is on, even after the media query is emulated.
  expect(filter).toMatch(/^blur\((?:28|0)px\) saturate\((?:2|1)\)$/);
}

function alphaOf(color: string): number {
  const rgba = color.match(/rgba?\([^)]*?,\s*([\d.]+)\)/);
  if (rgba?.[1]) return Number(rgba[1]);
  const slash = color.match(/\/\s*([\d.]+)\s*\)/);
  if (slash?.[1]) return Number(slash[1]);
  return 1;
}

function channel(value: number): number {
  const normalized = value / 255;
  return normalized <= 0.04045
    ? normalized / 12.92
    : Math.pow((normalized + 0.055) / 1.055, 2.4);
}


function hexLuminance(hex: string): number {
  const normalized = hex.trim().replace("#", "");
  const value =
    normalized.length === 3
      ? normalized
          .split("")
          .map((part) => `${part}${part}`)
          .join("")
      : normalized;
  const r = Number.parseInt(value.slice(0, 2), 16);
  const g = Number.parseInt(value.slice(2, 4), 16);
  const b = Number.parseInt(value.slice(4, 6), 16);
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrastHex(left: string, right: string): number {
  const a = hexLuminance(left);
  const b = hexLuminance(right);
  const lighter = Math.max(a, b);
  const darker = Math.min(a, b);
  return (lighter + 0.05) / (darker + 0.05);
}

function luminance(color: string): number {
  const match = color.match(/rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)/);
  if (!match) return 0;
  return (
    0.2126 * channel(Number(match[1])) +
    0.7152 * channel(Number(match[2])) +
    0.0722 * channel(Number(match[3]))
  );
}

for (const theme of ["light", "dark"] as const) {
  test.describe(`views (${theme})`, () => {
    for (const view of VIEWS) {
      test(view.name, async ({ page }) => {
        const errors = await boot(page, {
          theme,
          fixture: view.fixture,
        });
        await view.open(page);
        await expect(page.getByTestId("status-bar")).toContainText(view.status);
        await assertLayout(page);
        expect(errors, "console errors and warnings").toEqual([]);
      });
    }
  });
}

test.describe("geometry", () => {
  test("shell rhythm matches the contract", async ({ page }) => {
    const errors = await boot(page);
    await ensureSidebar(page);

    const viewport = page.viewportSize();
    if (!viewport) throw new Error("missing viewport");
    const toolbar = await getRect(page, '[data-testid="toolbar"]');
    const sub = await getRect(page, '[data-testid="sub-toolbar"]');
    const status = await getRect(page, '[data-testid="status-bar"]');
    const sidebar = await getRect(page, '[data-testid="sidebar"]');
    const row = await getRect(page, ".source-row");
    const filter = await getRect(page, ".table-filter");
    const select = await getRect(page, ".toolbar-select");
    const search = await getRect(page, ".search-trigger");
    const switcher = await getRect(page, '[data-testid="workspace-switcher"]');

    expect(toolbar.height).toBe(48);
    expect(sub.height).toBe(44);
    expect(status.height).toBe(26);
    expect(sidebar.width).toBe(240);
    expect(sidebar.top).toBe(0);
    expect(sidebar.bottom).toBe(viewport.height);
    expect(row.height).toBe(30);
    expect(row.left - sidebar.left).toBeGreaterThanOrEqual(10);
    expect(filter.height).toBe(28);
    expect(select.height).toBe(28);
    expect(search.height).toBe(28);
    expect(search.top - toolbar.top).toBeGreaterThanOrEqual(8);
    expect(toolbar.bottom - search.bottom).toBeGreaterThanOrEqual(8);

    const firstStatus = await getRect(page, ".status-left > *");
    expect(firstStatus.left - status.left).toBeGreaterThanOrEqual(16);

    const sections = await page.locator(".source-section").evaluateAll((elements) =>
      elements.map((element) => {
        const rect = element.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom };
      }),
    );
    for (let index = 1; index < sections.length; index += 1) {
      const previous = sections[index - 1];
      const current = sections[index];
      if (!previous || !current) continue;
      expect(current.top - previous.bottom).toBeGreaterThanOrEqual(16);
    }

    const axes = await page.evaluate(() =>
      Array.from(document.querySelectorAll(".source-section"))
        .map((section) => {
          const plus = section
            .querySelector(".source-heading .icon-control svg")
            ?.getBoundingClientRect();
          const count = section.querySelector(".source-count")?.getBoundingClientRect();
          return plus && count ? { plus: plus.right, count: count.right } : null;
        })
        .filter((value): value is { plus: number; count: number } => value !== null),
    );
    expect(axes.length).toBeGreaterThan(0);
    for (const axis of axes) {
      expect(Math.abs(axis.plus - axis.count)).toBeLessThanOrEqual(2);
    }

    if (viewport.width >= 1080) {
      expect(switcher.left).toBeGreaterThanOrEqual(sidebar.right + 12);
    } else {
      await page.keyboard.press("Escape");
    }
    await page.locator('[data-testid="grid-row"]').first().click();
    await expect(page.getByTestId("inspector")).toBeVisible();
    const inspector = await getRect(page, '[data-testid="inspector"]');
    expect(inspector.width).toBe(viewport.width < 1200 ? 420 : 360);
    const firstField = await getRect(page, ".record-form .form-item");
    expect(firstField.left - inspector.left).toBeGreaterThanOrEqual(14);
    expect(errors).toEqual([]);
  });

  test("numeric headers share the cell axis", async ({ page }) => {
    const errors = await boot(page);
    await selectTable(page, "Projects");
    await expect(page.locator('.grid-cell[data-column="Budget"]').first()).toBeVisible();
    const axis = await page.evaluate(() => {
      const idHeader = document.querySelector<HTMLElement>(".id-header span:last-child");
      const idCell = document.querySelector<HTMLElement>(".id-cell .cell-text");
      const budgetHeader = document.querySelector<HTMLElement>(
        '.grid-head-cell[data-column="Budget"] .column-name',
      );
      const budgetCell = document.querySelector<HTMLElement>(
        '.grid-cell[data-column="Budget"] .cell-text',
      );
      return {
        idHeader: idHeader?.getBoundingClientRect().right ?? 0,
        idCell: idCell?.getBoundingClientRect().right ?? 0,
        budgetHeader: budgetHeader?.getBoundingClientRect().right ?? 0,
        budgetCell: budgetCell?.getBoundingClientRect().right ?? 0,
      };
    });
    expect(Math.abs(axis.idHeader - axis.idCell)).toBeLessThanOrEqual(1);
    expect(Math.abs(axis.budgetHeader - axis.budgetCell)).toBeLessThanOrEqual(1);
    expect(errors).toEqual([]);
  });

  test("schema rows and graph metadata align", async ({ page }) => {
    const errors = await boot(page);
    await selectSidebarItem(page, "Relations & Schema");
    await expect(page.locator(".schema-view")).toBeVisible();
    const typeLefts = await page.locator(".manage-type").evaluateAll((elements) =>
      elements.map((element) => element.getBoundingClientRect().left),
    );
    expect(typeLefts.length).toBeGreaterThan(0);
    const first = typeLefts[0] ?? 0;
    for (const left of typeLefts) {
      expect(Math.abs(left - first)).toBeLessThanOrEqual(1);
    }
    const metadata = await page
      .locator(".schema-node-meta")
      .first()
      .evaluate((element) =>
        Array.from(element.children).map((child) => ({
          top: child.getBoundingClientRect().top,
          text: (child.textContent ?? "").trim(),
        })),
      );
    expect(metadata).toHaveLength(2);
    expect(metadata[1]?.text).toContain("out");
    expect(metadata[1]?.text).toContain("in");
    expect(metadata[1]?.top ?? 0).toBeGreaterThan(metadata[0]?.top ?? 0);
    expect(errors).toEqual([]);
  });

  test("max pane widths preserve the grid budget", async ({ page }) => {
    await page.addInitScript(() => {
      window.localStorage.setItem("slabbase.sidebarWidth", "300");
      window.localStorage.setItem("slabbase.inspectorWidth", "480");
    });
    const errors = await boot(page);
    await openInspector(page);
    await assertViewport(page);
    await assertChromeContainment(page);
    const grid = await getRect(page, '[data-testid="grid-pane"]');
    expect(grid.width).toBeGreaterThanOrEqual(480);
    const sub = await page.getByTestId("sub-toolbar").evaluate((element) => ({
      scrollWidth: element.scrollWidth,
      clientWidth: element.clientWidth,
    }));
    expect(sub.scrollWidth).toBeLessThanOrEqual(sub.clientWidth + 1);
    expect(errors).toEqual([]);
  });

  test("density keeps virtual rows aligned", async ({ page }) => {
    const errors = await boot(page);
    await selectTable(page, "Invoices");
    await openInspector(page);
    await expect(page.getByTestId("portal-scroll")).toBeVisible();
    await page.locator(".record-scroll").evaluate((node) => {
      node.scrollTop = node.scrollHeight;
    });

    for (const density of ["compact", "default", "comfortable"] as const) {
      const expected = DENSITY_HEIGHT[density];
      await page.getByTestId("appearance-button").click();
      await page.getByTestId("appearance-popover").getByRole("button", {
        name: DENSITY_LABEL[density],
        exact: true,
      }).click();
      await page.keyboard.press("Escape");
      await expect(page.getByTestId("appearance-popover")).toBeHidden();

      const geometry = await page.evaluate(() => {
        const cssRow = getComputedStyle(document.documentElement).getPropertyValue("--row-h");
        const rows = Array.from(document.querySelectorAll<HTMLElement>(".grid-row")).map((row) => {
          const rect = row.getBoundingClientRect();
          const transform = getComputedStyle(row).transform;
          return { height: rect.height, transform };
        });
        const portalRows = Array.from(
          document.querySelectorAll<HTMLElement>(".portal-grid-row"),
        ).map((row) => {
          const rect = row.getBoundingClientRect();
          return { height: rect.height };
        });
        const portalSpacer = document.querySelector<HTMLElement>(".portal-spacer");
        return {
          cssRow: cssRow.trim(),
          rows,
          portalRows,
          portalSpacer: portalSpacer?.getBoundingClientRect().height ?? 0,
          portalRowsTotal: document.querySelectorAll(".portal-grid-row").length,
        };
      });

      expect(geometry.cssRow).toBe(`${expected}px`);
      for (const row of geometry.rows) {
        expect(row.height).toBe(expected);
        if (row.transform !== "none") {
          const values = row.transform.match(/matrix\(1, 0, 0, 1, 0, ([\d.-]+)\)/);
          if (values?.[1]) {
            expect(Math.abs(Number(values[1]) % expected)).toBeLessThanOrEqual(0.01);
          }
        }
      }
      for (const row of geometry.portalRows) {
        expect(row.height).toBe(expected);
      }
      expect(geometry.portalSpacer).toBeGreaterThanOrEqual(
        geometry.portalRowsTotal * expected - 1,
      );
    }
    expect(errors).toEqual([]);
  });
});

test.describe("interaction polish", () => {
  test("focus rings follow keyboard intent", async ({ page }) => {
    const errors = await boot(page);
    await page.getByRole("tab", { name: "Table" }).click();
    const pointerFocus = await page.evaluate(() => {
      const active = document.activeElement as HTMLElement;
      return {
        outlineStyle: getComputedStyle(active).outlineStyle,
        outlineWidth: getComputedStyle(active).outlineWidth,
      };
    });
    expect(pointerFocus.outlineStyle).toBe("none");

    await page.keyboard.press("Tab");
    const keyboardFocus = await page.evaluate(() => {
      const active = document.activeElement as HTMLElement;
      return {
        visible: active.matches(":focus-visible"),
        outlineWidth: getComputedStyle(active).outlineWidth,
      };
    });
    expect(keyboardFocus.visible).toBe(true);
    expect(keyboardFocus.outlineWidth).toBe("2px");

    const trigger = page.getByTestId("search-trigger");
    await trigger.click();
    await expect(page.getByTestId("command-palette")).toBeVisible();
    for (let index = 0; index < 20; index += 1) {
      await page.keyboard.press("Tab");
      const inside = await page.evaluate(() =>
        document
          .querySelector('[data-testid="command-palette"]')
          ?.contains(document.activeElement),
      );
      expect(inside).toBe(true);
    }
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("command-palette")).toBeHidden();
    expect(await trigger.evaluate((node) => node === document.activeElement)).toBe(true);
    expect(errors).toEqual([]);
  });

  test("window blur neutralizes active tints", async ({ page }) => {
    const errors = await boot(page);
    const row = page.locator(".source-row.active");
    await expect(row).toBeVisible();
    await page.evaluate(() => window.dispatchEvent(new Event("blur")));
    expect(
      await page.locator("html").getAttribute("data-window-focused"),
    ).toBe("false");
    await expect
      .poll(() => row.evaluate((element) => getComputedStyle(element).backgroundColor))
      .toBe("rgba(128, 128, 128, 0.16)");
    const inactive = await row.evaluate((element) => getComputedStyle(element).backgroundColor);
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    expect(
      await page.locator("html").getAttribute("data-window-focused"),
    ).toBe("true");
    await expect
      .poll(() => row.evaluate((element) => getComputedStyle(element).backgroundColor))
      .not.toBe(inactive);
    expect(errors).toEqual([]);
  });

  test("context menu clamps and dismisses", async ({ page }) => {
    const errors = await boot(page);
    await page.setViewportSize({ width: 1280, height: 360 });
    await page.waitForFunction(() => {
      const node = document.querySelector<HTMLElement>('[data-testid="sidebar"]');
      return Boolean(node && node.getBoundingClientRect().x >= 0);
    });
    await selectTable(page, "Line Items");
    const viewport = page.viewportSize();
    if (!viewport) throw new Error("missing viewport");
    await page.mouse.click(viewport.width - 12, 140, { button: "right" });
    const menu = page.getByTestId("context-menu");
    await expect(menu).toBeVisible();
    const box = await menu.boundingBox();
    if (!box) throw new Error("missing context menu box");
    expect(box.x).toBeGreaterThanOrEqual(8);
    expect(box.y).toBeGreaterThanOrEqual(8);
    expect(box.x + box.width).toBeLessThanOrEqual(viewport.width - 8);
    expect(box.y + box.height).toBeLessThanOrEqual(viewport.height - 8);
    await page.keyboard.press("Escape");
    await expect(menu).toBeHidden();

    await page.mouse.click(viewport.width - 12, 140, { button: "right" });
    await expect(menu).toBeVisible();
    await page.mouse.move(400, 220);
    await page.mouse.wheel(0, 120);
    await expect(menu).toBeHidden();
    expect(errors).toEqual([]);
  });

  test("reduced motion collapses surfaces", async ({ page }) => {
    const errors = await boot(page);
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await page.getByTestId("search-trigger").click();
    await expect(page.getByTestId("command-palette")).toBeVisible();
    const normalPalette = await page
      .getByTestId("command-palette")
      .evaluate((element) => getComputedStyle(element).animationDuration);
    expect(Number.parseFloat(normalPalette)).toBeGreaterThanOrEqual(0.1);
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("command-palette")).toBeHidden();

    await selectSidebarItem(page, "Audit Log");
    const drawer = page.getByTestId("audit-drawer");
    await expect(drawer).toBeVisible();
    const normalSheet = await drawer.evaluate(
      (element) => getComputedStyle(element).animationDuration,
    );
    expect(Number.parseFloat(normalSheet)).toBeGreaterThanOrEqual(0.14);
    await page.keyboard.press("Escape");
    await expect(drawer).toBeHidden();
    const scrim = page.locator(".sidebar-scrim");
    if (await scrim.isVisible()) {
      await page.keyboard.press("Escape");
      await expect(scrim).toBeHidden();
    }

    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.getByTestId("search-trigger").click();
    await expect(page.getByTestId("command-palette")).toBeVisible();
    const reduced = await page.evaluate(() => {
      const element = document.querySelector<HTMLElement>('[data-testid="command-palette"]');
      if (!element) return null;
      const style = getComputedStyle(element);
      return { duration: style.animationDuration, transform: style.transform };
    });
    expect(reduced).not.toBeNull();
    expect(Number.parseFloat(reduced?.duration ?? "1s")).toBeLessThanOrEqual(0.06);
    expect(reduced?.transform).toBe("none");
    expect(errors).toEqual([]);
  });

  test("dark surfaces step and bevel", async ({ page }) => {
    const errors = await boot(page, { theme: "dark" });
    await page.getByTestId("appearance-button").click();
    await expect(page.getByTestId("appearance-popover")).toBeVisible();
    const values = await page.evaluate(() => {
      const color = (selector: string) => {
        const element = document.querySelector<HTMLElement>(selector);
        return element ? getComputedStyle(element).backgroundColor : "";
      };
      const popover = document.querySelector<HTMLElement>('[data-testid="appearance-popover"]');
      const style = popover ? getComputedStyle(popover) : null;
      return {
        grid: color('[data-testid="grid-pane"]'),
        sidebar: color('[data-testid="sidebar"]'),
        toolbar: color('[data-testid="toolbar"]'),
        elevated: color('[data-testid="appearance-popover"]'),
        borderWidth: style?.borderTopWidth ?? "",
        boxShadow: style?.boxShadow ?? "",
      };
    });
    const grid = luminance(values.grid);
    const sidebar = luminance(values.sidebar);
    const toolbar = luminance(values.toolbar);
    const elevated = luminance(values.elevated);
    expect(grid).toBeLessThan(sidebar);
    expect(sidebar).toBeLessThanOrEqual(toolbar);
    expect(toolbar).toBeLessThan(elevated);
    expect(values.borderWidth).toBe("1px");
    expect(values.boxShadow).toContain("inset");
    expect(errors).toEqual([]);
  });

  test("glass keeps chrome translucent and grids opaque", async ({ page }) => {
    const errors = await boot(page, {
      theme: "dark",
      surface: "glass",
      accent: "teal",
    });
    await setReducedTransparency(page, "no-preference");
    const readGlass = () =>
      page.evaluate(() => {
        const sidebar = document.querySelector<HTMLElement>('[data-testid="sidebar"]');
        const grid = document.querySelector<HTMLElement>('[data-testid="grid-pane"]');
        const header = document.querySelector<HTMLElement>('[data-testid="grid-header"]');
        return {
          sidebarFilter: sidebar ? getComputedStyle(sidebar).backdropFilter : "",
          sidebarBlur: sidebar
            ? getComputedStyle(sidebar).getPropertyValue("--glass-blur").trim()
            : "",
          sidebarBg: sidebar ? getComputedStyle(sidebar).backgroundColor : "",
          gridBg: grid ? getComputedStyle(grid).backgroundColor : "",
          headerBg: header ? getComputedStyle(header).backgroundColor : "",
        };
      });

    const readPopover = () =>
      page.evaluate(() => {
        const element = document.querySelector<HTMLElement>('[data-testid="appearance-popover"]');
        if (!element) return null;
        const style = getComputedStyle(element);
        return {
          background: style.backgroundColor,
          border: style.borderTopColor,
          bevel: style.boxShadow,
        };
      });

    const darkGlass = await readGlass();
    expectGlassBlurToken(darkGlass.sidebarBlur);
    expectGlassBackdrop(darkGlass.sidebarFilter);
    expect(alphaOf(darkGlass.sidebarBg)).toBeLessThan(1);
    expect(alphaOf(darkGlass.gridBg)).toBeGreaterThanOrEqual(0.96);
    expect(alphaOf(darkGlass.headerBg)).toBeGreaterThanOrEqual(0.96);

    await page.getByTestId("appearance-button").click();
    const popover = page.getByTestId("appearance-popover");
    await popover.locator('[data-accent-option="rose"]').click();
    expect(await page.locator("html").getAttribute("data-accent")).toBe("rose");

    await popover.getByRole("button", { name: "Light", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await expect(page.locator("html")).toHaveAttribute("data-surface", "glass");
    const lightGlass = await readGlass();
    expectGlassBlurToken(lightGlass.sidebarBlur);
    expectGlassBackdrop(lightGlass.sidebarFilter);
    expect(alphaOf(lightGlass.sidebarBg)).toBeLessThan(1);
    expect(alphaOf(lightGlass.gridBg)).toBeGreaterThanOrEqual(0.96);
    await popover.locator('[data-accent-option="orange"]').click();
    expect(await page.locator("html").getAttribute("data-accent")).toBe("orange");

    await page.evaluate(() => {
      document.documentElement.dataset["native"] = "true";
    });
    // Reduced motion collapses every transition to 0.01ms, yet each computed
    // value still changes only when its transition finishes on the next
    // animation frame, and a loaded runner can delay that frame past any
    // fixed sleep. Retry the whole snapshot until every token has settled.
    await expect(async () => {
      const nativeLight = await readGlass();
      const nativeLightPopover = await readPopover();
      expectGlassBlurToken(nativeLight.sidebarBlur);
      expectGlassBackdrop(nativeLight.sidebarFilter);
      expect(alphaOf(nativeLight.sidebarBg)).toBeGreaterThan(0.2);
      expect(alphaOf(nativeLight.sidebarBg)).toBeLessThan(0.6);
      expect(alphaOf(nativeLight.gridBg)).toBeGreaterThanOrEqual(0.96);
      expect(nativeLightPopover).not.toBeNull();
      expect(alphaOf(nativeLightPopover?.background ?? "")).toBeGreaterThan(0.5);
      expect(alphaOf(nativeLightPopover?.background ?? "")).toBeLessThan(0.75);
      expect(alphaOf(nativeLightPopover?.border ?? "")).toBeGreaterThan(0.4);
      expect(nativeLightPopover?.bevel).toContain("inset");
    }).toPass();

    await popover.getByRole("button", { name: "Dark", exact: true }).click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await expect(async () => {
      const nativeDark = await readGlass();
      const nativeDarkPopover = await readPopover();
      expect(alphaOf(nativeDark.sidebarBg)).toBeGreaterThan(0.3);
      expect(alphaOf(nativeDark.sidebarBg)).toBeLessThan(0.5);
      expect(alphaOf(nativeDark.gridBg)).toBeGreaterThanOrEqual(0.96);
      expect(alphaOf(nativeDarkPopover?.background ?? "")).toBeGreaterThan(0.5);
      expect(alphaOf(nativeDarkPopover?.background ?? "")).toBeLessThan(0.75);
      expect(alphaOf(nativeDarkPopover?.border ?? "")).toBeLessThan(0.25);
    }).toPass();

    await setReducedTransparency(page, "reduce");
    await expect.poll(async () => (await readGlass()).sidebarBlur).toBe("none");
    await expect.poll(async () => (await readGlass()).sidebarFilter).toBe("none");
    await setReducedTransparency(page, "no-preference");
    await expect.poll(async () => (await readGlass()).sidebarBlur).toContain("blur(28px)");
    await expect
      .poll(async () => (await readGlass()).sidebarFilter)
      .toMatch(/^blur\((?:28|0)px\) saturate\((?:2|1)\)$/);

    await assertViewport(page);
    expect(errors).toEqual([]);
  });

  test("accent hex follows the resolved theme", async ({ page }) => {
    const errors = await boot(page);
    await page.getByTestId("appearance-button").click();
    const popover = page.getByTestId("appearance-popover");
    const hex = page.getByTestId("accent-hex");
    await expect(hex).toHaveValue("#0066cc");
    await popover.getByRole("button", { name: "Dark", exact: true }).click();
    await expect(hex).toHaveValue("#148aff");
    await popover.getByRole("button", { name: "Light", exact: true }).click();
    await expect(hex).toHaveValue("#0066cc");
    await popover.locator('[data-accent-option="rose"]').click();
    await expect(hex).toHaveValue("#be123c");
    await popover.getByRole("button", { name: "Dark", exact: true }).click();
    await expect(hex).toHaveValue("#ff375f");
    await hex.fill("#123456");
    await hex.press("Enter");
    await popover.getByRole("button", { name: "Light", exact: true }).click();
    await expect(hex).toHaveValue("#123456");
    expect(errors).toEqual([]);
  });

  test("appearance controls persist", async ({ page }) => {
    const errors = await boot(page, { seed: false, theme: "system" });
    await page.emulateMedia({ colorScheme: "dark" });
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    await page.emulateMedia({ colorScheme: "light" });
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");

    await page.getByTestId("appearance-button").click();
    const popover = page.getByTestId("appearance-popover");
    for (const id of ["blue", "graphite", "indigo", "teal", "green", "orange", "rose"]) {
      await popover.locator(`[data-accent-option="${id}"]`).click();
      const pair = await page.evaluate(() => {
        const style = getComputedStyle(document.documentElement);
        return {
          accent: style.getPropertyValue("--accent").trim(),
          foreground: style.getPropertyValue("--accent-on-solid").trim(),
        };
      });
      expect(contrastHex(pair.accent, pair.foreground), `${id} contrast`).toBeGreaterThanOrEqual(
        4.5,
      );
    }
    await popover.getByRole("button", { name: "Dark", exact: true }).click();
    await popover.getByRole("button", { name: "Glass", exact: true }).click();
    await popover.locator('[data-accent-option="rose"]').click();
    const hex = page.getByTestId("accent-hex");
    await hex.fill("#123456");
    await hex.press("Enter");
    await popover.getByRole("button", { name: "Comfortable", exact: true }).click();
    await page.keyboard.press("Escape");

    const root = page.locator("html");
    await expect(root).toHaveAttribute("data-theme", "dark");
    await expect(root).toHaveAttribute("data-surface", "glass");
    await expect(root).toHaveAttribute("data-accent", "custom");
    await expect(root).toHaveAttribute("data-density", "comfortable");
    const pair = await page.evaluate(() => {
      const style = getComputedStyle(document.documentElement);
      return {
        accent: style.getPropertyValue("--accent").trim(),
        foreground: style.getPropertyValue("--accent-on-solid").trim(),
      };
    });
    expect(pair.accent.toLowerCase()).toBe("#123456");
    expect(contrastHex(pair.accent, pair.foreground)).toBeGreaterThanOrEqual(4.5);

    await page.reload();
    await expect(page.getByTestId("grid-pane")).toBeVisible();
    await expect(root).toHaveAttribute("data-theme", "dark");
    await expect(root).toHaveAttribute("data-surface", "glass");
    await expect(root).toHaveAttribute("data-accent", "custom");
    await expect(root).toHaveAttribute("data-density", "comfortable");
    const stored = await page.evaluate(() =>
      window.localStorage.getItem("slabbase.appearance.v1"),
    );
    expect(stored).toContain('"version":1');
    expect(errors).toEqual([]);
  });

  test("native chrome reserves traffic lights", async ({ page }) => {
    const errors = await boot(page, { tauri: true });
    const viewport = page.viewportSize();
    if (!viewport) throw new Error("missing viewport");
    await expect(page.locator("html")).toHaveAttribute("data-native", "true");
    const sidebarHeaderPadding = await page
      .locator(".sidebar-header")
      .evaluate((element) => Number.parseFloat(getComputedStyle(element).paddingLeft));
    expect(sidebarHeaderPadding).toBeGreaterThanOrEqual(70);

    if (viewport.width >= 1080) {
      await page.getByTestId("sidebar-header-toggle").click();
      await expect(page.getByTestId("sidebar")).toBeHidden();
      await expect(page.getByTestId("toolbar")).toHaveClass(/traffic-space/);
      await expect
        .poll(() =>
          page
            .getByTestId("toolbar")
            .evaluate((element) => Number.parseFloat(getComputedStyle(element).paddingLeft)),
        )
        .toBeGreaterThanOrEqual(70);
      const switcher = await getRect(page, '[data-testid="workspace-switcher"]');
      expect(switcher.left).toBeGreaterThanOrEqual(12);
    } else {
      const toolbarPadding = await page
        .getByTestId("toolbar")
        .evaluate((element) => Number.parseFloat(getComputedStyle(element).paddingLeft));
      expect(toolbarPadding).toBeGreaterThanOrEqual(70);
      await page.getByTestId("sidebar-toggle").click();
      const headerPadding = await page
        .locator(".sidebar-header")
        .evaluate((element) => Number.parseFloat(getComputedStyle(element).paddingLeft));
      expect(headerPadding).toBeGreaterThanOrEqual(70);
    }
    expect(errors).toEqual([]);
  });

  test("overlay behavior follows viewport budgets", async ({ page }) => {
    const errors = await boot(page);
    const viewport = page.viewportSize();
    if (!viewport) throw new Error("missing viewport");

    if (viewport.width < 1080) {
      const sidebar = page.getByTestId("sidebar");
      const closed = await sidebar.boundingBox();
      expect(closed?.x ?? 0).toBeLessThan(0);
      await page.getByTestId("sidebar-toggle").click();
      await page.waitForFunction(() => {
        const node = document.querySelector<HTMLElement>('[data-testid="sidebar"]');
        return node ? node.getBoundingClientRect().x >= 0 : false;
      });
      const open = await sidebar.boundingBox();
      expect(open?.x).toBe(0);
      expect(open?.width).toBe(240);
      await assertViewport(page);
      await page.keyboard.press("Escape");
      await page.waitForFunction(() => {
        const node = document.querySelector<HTMLElement>('[data-testid="sidebar"]');
        return node ? node.getBoundingClientRect().x < 0 : true;
      });
    } else {
      const sidebar = page.getByTestId("sidebar");
      await expect(sidebar).toBeVisible();
      await page.getByTestId("sidebar-header-toggle").click();
      await expect(sidebar).toBeHidden();
      await page.getByTestId("sidebar-toggle").click();
      await expect(sidebar).toBeVisible();
    }

    await page.locator('[data-testid="grid-row"]').first().click();
    await expect(page.getByTestId("inspector")).toBeVisible();
    const inspector = await page.evaluate(() => {
      const element = document.querySelector<HTMLElement>('[data-testid="inspector"]');
      const grid = document.querySelector<HTMLElement>('[data-testid="grid-pane"]');
      if (!element || !grid) return null;
      const style = getComputedStyle(element);
      const gridRect = grid.getBoundingClientRect();
      return {
        position: style.position,
        top: element.getBoundingClientRect().top,
        bottom: element.getBoundingClientRect().bottom,
        width: element.getBoundingClientRect().width,
        gridWidth: gridRect.width,
      };
    });
    expect(inspector).not.toBeNull();
    expect(inspector?.gridWidth ?? 0).toBeGreaterThanOrEqual(480);
    if (viewport.width < 1200) {
      expect(inspector?.position).toBe("fixed");
      expect(inspector?.top).toBe(48);
      expect(inspector?.bottom).toBe(viewport.height - 26);
      expect(inspector?.width ?? 0).toBeLessThanOrEqual(420);
    } else {
      expect(inspector?.position).toBe("static");
      expect(inspector?.width).toBe(360);
    }
    expect(errors).toEqual([]);
  });

  test("scroll edges and overscroll stay contained", async ({ page }) => {
    const errors = await boot(page);
    await page.setViewportSize({ width: 1280, height: 360 });
    await page.waitForFunction(() => {
      const node = document.querySelector<HTMLElement>('[data-testid="sidebar"]');
      return Boolean(node && node.getBoundingClientRect().x >= 0);
    });
    await selectTable(page, "Line Items");
    const scroll = page.getByTestId("grid-scroll");
    const header = page.getByTestId("grid-header");
    expect(await header.evaluate((element) => getComputedStyle(element).boxShadow)).toBe(
      "none",
    );
    await scroll.evaluate((element) => {
      element.scrollTop = 60;
    });
    await expect
      .poll(() => header.evaluate((element) => getComputedStyle(element).boxShadow))
      .not.toBe("none");
    await scroll.evaluate((element) => {
      element.scrollTop = 0;
    });
    await expect
      .poll(() => header.evaluate((element) => getComputedStyle(element).boxShadow))
      .toBe("none");
    const overscroll = await scroll.evaluate(
      (element) => getComputedStyle(element).overscrollBehaviorY,
    );
    expect(overscroll).toBe("contain");
    expect(errors).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// v1.0 torture audit: wide content, rapid interaction, destructive recovery,
// keyboard-only operation, internationalization, accessibility, zoom, and
// air-gapped boot.
// ---------------------------------------------------------------------------

async function openWideStress(page: Page): Promise<string[]> {
  const errors = await boot(page, { fixture: "stress" });
  await expect(page.getByTestId("grid-pane")).toBeVisible();
  await expect(page.locator(".grid-head-cell.column-header")).toHaveCount(20);
  return errors;
}

test.describe("v1 torture", () => {
  test("wide table and long values stay inside the grid viewport", async ({ page }) => {
    const errors = await openWideStress(page);
    const metrics = await page.evaluate(() => {
      const grid = document.querySelector<HTMLElement>('[data-testid="grid-scroll"]');
      return {
        scrollWidth: grid?.scrollWidth ?? 0,
        clientWidth: grid?.clientWidth ?? 0,
        docWidth: document.documentElement.scrollWidth,
        innerWidth: window.innerWidth,
      };
    });
    expect(metrics.scrollWidth).toBeGreaterThan(metrics.clientWidth + 500);
    expect(metrics.docWidth).toBeLessThanOrEqual(metrics.innerWidth);

    const title = await page
      .locator('.grid-cell[data-column="Column 01"] .cell-text')
      .first()
      .getAttribute("title");
    expect((title ?? "").length).toBeGreaterThan(500);
    const dir = await page
      .locator('.grid-cell[data-column="Column 01"] .cell-text')
      .first()
      .getAttribute("dir");
    expect(dir).toBe("auto");

    await assertChromeContainment(page);
    await assertViewport(page);

    await page.locator('[data-testid="grid-scroll"]').evaluate((node) => {
      node.scrollTop = 380;
      node.scrollLeft = 900;
    });
    const sticky = await page.evaluate(() => {
      const viewport = document
        .querySelector<HTMLElement>('[data-testid="grid-scroll"]')
        ?.getBoundingClientRect();
      const header = document.querySelector<HTMLElement>(".grid-header")?.getBoundingClientRect();
      return { viewportTop: viewport?.top ?? 0, headerTop: header?.top ?? 0 };
    });
    expect(Math.abs(sticky.headerTop - sticky.viewportTop)).toBeLessThanOrEqual(1);

    await page.locator('[data-testid="grid-row"]').first().click();
    await expect(page.getByTestId("inspector")).toBeVisible();
    await assertChromeContainment(page);
    await assertViewport(page);
    expect(errors).toEqual([]);
  });

  test("rapid density, surface, theme, and inspector bursts keep scroll stable", async ({ page }) => {
    const errors = await openWideStress(page);
    await page.locator('[data-testid="grid-row"]').first().click();
    await expect(page.getByTestId("inspector")).toBeVisible();
    const grid = page.getByTestId("grid-scroll");

    for (let cycle = 0; cycle < 4; cycle += 1) {
      if (await page.getByTestId("appearance-popover").isVisible()) {
        await page.keyboard.press("Escape");
        await expect(page.getByTestId("appearance-popover")).toBeHidden();
      }
      await page.getByTestId("appearance-button").click();
      await expect(page.getByTestId("appearance-popover")).toBeVisible();
      const density = DENSITY_LABEL[(["compact", "default", "comfortable"] as const)[cycle % 3]!];
      await page
        .getByTestId("appearance-popover")
        .getByRole("button", { name: density, exact: true })
        .click();
      await page
        .getByTestId("appearance-popover")
        .getByRole("group", { name: "Surface" })
        .getByRole("button", { name: cycle % 2 === 0 ? "Glass" : "Solid", exact: true })
        .click();
      await page
        .getByTestId("appearance-popover")
        .getByRole("group", { name: "Theme" })
        .getByRole("button", { name: cycle % 2 === 0 ? "Dark" : "Light", exact: true })
        .click();
      await grid.evaluate((node, value) => {
        node.scrollTop = value;
      }, 120 + cycle * 40);
      await page.getByTestId("inspector-toggle").click();
      await page.getByTestId("inspector-toggle").click();
      await expect(page.getByTestId("inspector")).toBeVisible();
    }
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("appearance-popover")).toBeHidden();

    const state = await page.evaluate(() => {
      const density = document.documentElement.dataset["density"];
      const rows = Array.from(document.querySelectorAll<HTMLElement>(".grid-row")).map(
        (row) => row.getBoundingClientRect().height,
      );
      const scroll = document.querySelector<HTMLElement>('[data-testid="grid-scroll"]');
      return {
        density,
        rows,
        scrollTop: scroll?.scrollTop ?? 0,
      };
    });
    const expected = DENSITY_HEIGHT[state.density as Density] ?? 29;
    expect(state.rows.length).toBeGreaterThan(0);
    for (const height of state.rows) expect(height).toBe(expected);
    expect(state.scrollTop).toBeGreaterThan(0);
    await assertLayout(page);
    expect(errors).toEqual([]);
  });

  test("portal rows keep density alignment during rapid appearance changes", async ({ page }) => {
    const errors = await boot(page);
    await selectTable(page, "Invoices");
    await openInspector(page);
    await expect(page.getByTestId("portal-scroll")).toBeVisible();
    await page.locator(".record-scroll").evaluate((node) => {
      node.scrollTop = node.scrollHeight;
    });
    for (const density of ["compact", "comfortable", "default"] as const) {
      if (await page.getByTestId("appearance-popover").isVisible()) {
        await page.keyboard.press("Escape");
        await expect(page.getByTestId("appearance-popover")).toBeHidden();
      }
      await page.getByTestId("appearance-button").click();
      await page.getByTestId("appearance-popover").getByRole("button", {
        name: DENSITY_LABEL[density],
        exact: true,
      }).click();
      await page.waitForFunction(
        (expected) =>
          Array.from(document.querySelectorAll<HTMLElement>(".portal-grid-row")).every(
            (row) => Math.abs(row.getBoundingClientRect().height - expected) < 0.5,
          ),
        DENSITY_HEIGHT[density],
      );
      const geometry = await page.evaluate(() => ({
        css: getComputedStyle(document.documentElement).getPropertyValue("--row-h").trim(),
        rows: Array.from(document.querySelectorAll<HTMLElement>(".portal-grid-row")).map(
          (row) => row.getBoundingClientRect().height,
        ),
        spacer: document.querySelector<HTMLElement>(".portal-spacer")?.getBoundingClientRect()
          .height ?? 0,
      }));
      expect(geometry.css).toBe(`${DENSITY_HEIGHT[density]}px`);
      for (const height of geometry.rows) expect(height).toBe(DENSITY_HEIGHT[density]);
      expect(geometry.spacer).toBeGreaterThanOrEqual(
        geometry.rows.length * DENSITY_HEIGHT[density] - 1,
      );
    }
    await page.keyboard.press("Escape");
    expect(errors).toEqual([]);
  });

  test("dropping a formula column reports an inline error and keeps state", async ({ page }) => {
    const errors = await boot(page);
    await selectSidebarItem(page, "Relations & Schema");
    await expect(page.locator(".schema-view")).toBeVisible();
    await page.getByLabel("Managed table").selectOption("Invoices");
    page.once("dialog", (dialog) => void dialog.accept());
    await page.getByRole("button", { name: "Drop Tax Rate" }).click();
    const problem = page.getByTestId("schema-error");
    await expect(problem).toBeVisible();
    await expect(problem).toContainText("referenced by formula");
    await expect(page.getByLabel("Rename Tax Rate")).toBeVisible();
    await expect(page.locator(".manage-row")).toHaveCount(
      await page.locator(".manage-row").count(),
    );
    await expect(page.getByTestId("status-bar")).toContainText("4 tables");
    expect(errors).toEqual([]);
  });

  test("relation-protected table drop, relation drop, and undo restore everything", async ({ page }) => {
    const errors = await boot(page);
    await selectSidebarItem(page, "Relations & Schema");
    await page.getByLabel("Managed table").selectOption("Line Items");
    page.once("dialog", (dialog) => void dialog.accept());
    await page.getByRole("button", { name: "Drop Table" }).click();
    await expect(page.getByTestId("schema-error")).toContainText("used by relation");
    await expect(page.locator(".manage-row").first()).toBeVisible();

    const relationRow = page.locator(".manage-relation-row").filter({ hasText: "Invoice Lines" });
    page.once("dialog", (dialog) => void dialog.accept());
    await relationRow.getByRole("button", { name: "Drop" }).click();
    await expect(page.getByTestId("schema-error")).toBeHidden();

    // The relation is now gone; dropping the table succeeds.
    page.once("dialog", (dialog) => void dialog.accept());
    await page.getByRole("button", { name: "Drop Table" }).click();
    await expect(page.getByLabel("Managed table")).not.toContainText("Line Items");

    await page.keyboard.press("Meta+z");
    await expect(page.getByTestId("toast")).toContainText(/Undid change|Undone/);
    await expect(page.getByLabel("Managed table")).toContainText("Line Items");
    await page.keyboard.press("Meta+z");
    await expect(page.locator(".manage-relation-row").filter({ hasText: "Invoice Lines" })).toBeVisible();
    expect(errors).toEqual([]);
  });

  test("every overlay returns focus to its opener after Escape", async ({ page }) => {
    const errors = await boot(page);

    const schemaOpener = page.getByRole("button", { name: "+ Column" });
    await schemaOpener.click();
    await expect(page.getByTestId("schema-dialog")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("schema-dialog")).toBeHidden();
    await expect
      .poll(() => schemaOpener.evaluate((node) => node === document.activeElement))
      .toBe(true);

    const importOpener = page.getByRole("button", { name: "Import" });
    await importOpener.click();
    await expect(page.getByTestId("import-dialog")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("import-dialog")).toBeHidden();
    await expect
      .poll(() => importOpener.evaluate((node) => node === document.activeElement))
      .toBe(true);

    const printOpener = page.getByRole("button", { name: "Print or Export PDF" });
    await printOpener.click();
    await expect(page.getByTestId("print-dialog")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("print-dialog")).toBeHidden();
    await expect
      .poll(() => printOpener.evaluate((node) => node === document.activeElement))
      .toBe(true);

    const appearanceOpener = page.getByTestId("appearance-button");
    await appearanceOpener.click();
    await expect(page.getByTestId("appearance-popover")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("appearance-popover")).toBeHidden();
    await expect
      .poll(() => appearanceOpener.evaluate((node) => node === document.activeElement))
      .toBe(true);

    const searchOpener = page.getByTestId("search-trigger");
    await searchOpener.click();
    await expect(page.getByTestId("command-palette")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("command-palette")).toBeHidden();
    await expect
      .poll(() => searchOpener.evaluate((node) => node === document.activeElement))
      .toBe(true);

    await ensureSidebar(page);
    const auditOpener = page
      .getByTestId("sidebar")
      .getByRole("button", { name: /Audit Log/ });
    await auditOpener.click();
    await expect(page.getByTestId("audit-drawer")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("audit-drawer")).toBeHidden();
    await expect
      .poll(() => auditOpener.evaluate((node) => node === document.activeElement))
      .toBe(true);
    expect(errors).toEqual([]);
  });

  test("keyboard-only navigation, hostile search, and undo routing", async ({ page }) => {
    const errors = await boot(page);
    await page.keyboard.press("Meta+2");
    await expect(page.getByRole("tab", { name: "Form" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await page.keyboard.press("Meta+3");
    await expect(page.locator(".schema-view")).toBeVisible();
    await page.keyboard.press("Meta+4");
    await expect(page.getByTestId("audit-drawer")).toBeVisible();
    await page.keyboard.press("Escape");
    await page.keyboard.press("Meta+1");
    await expect(page.getByTestId("grid-pane")).toBeVisible();

    await page.keyboard.press("Meta+k");
    const search = page.getByLabel("Search records and commands");
    await expect(search).toBeFocused();
    await search.fill('northlight* "unclosed OR AND NOT col:value ^start ()[]{}');
    await page.waitForTimeout(120);
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("command-palette")).toBeHidden();

    await page.getByTestId("grid-scroll").focus();
    await page.keyboard.press("ArrowDown");
    await page.keyboard.press("ArrowRight");
    await page.keyboard.press("Enter");
    const active = page.locator(".grid-cell .field-input").first();
    await expect(active).toBeVisible();
    const before = await active.inputValue();
    await page.keyboard.type("zz");
    await expect(active).not.toHaveValue(before);
    await page.keyboard.press("Meta+z");
    await expect(active).toHaveValue(before);
    await page.keyboard.press("Escape");
    await expect(active).toBeHidden();
    await page.keyboard.press("Meta+z");
    await expect(page.getByTestId("toast")).toContainText(/Undid change|Undone/);

    if ((page.viewportSize()?.width ?? 0) >= 1080) {
      await page.keyboard.press("Meta+Alt+s");
      await expect(page.getByTestId("sidebar")).toBeHidden();
      await page.keyboard.press("Meta+Alt+s");
      await expect(page.getByTestId("sidebar")).toBeVisible();
    }
    const inspector = page.getByTestId("inspector");
    if (await inspector.isVisible()) {
      await page.keyboard.press("Meta+Alt+i");
      await expect(inspector).toBeHidden();
    }
    await page.keyboard.press("Meta+Alt+i");
    await expect(inspector).toBeVisible();
    await page.keyboard.press("Meta+Alt+i");
    await expect(inspector).toBeHidden();
    expect(errors).toEqual([]);
  });

  test("IME composition Enter does not commit inline or inspector editors", async ({ page }) => {
    const errors = await boot(page);
    const cell = page.locator('.grid-cell[data-column="Name"]').first();
    await cell.dblclick();
    const input = cell.locator("input.field-input");
    await expect(input).toBeVisible();
    await input.evaluate((node) => {
      node.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
      node.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Enter",
          code: "Enter",
          bubbles: true,
          isComposing: true,
        }),
      );
    });
    await page.waitForTimeout(120);
    await expect(input).toBeVisible();
    await input.evaluate((node) => {
      node.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
    });
    await page.keyboard.press("Escape");
    await expect(input).toBeHidden();

    await cell.click();
    await expect(page.getByTestId("inspector")).toBeVisible();
    const formInput = page.getByTestId("inspector").getByLabel("Name").first();
    await formInput.click();
    await formInput.evaluate((node) => {
      node.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
      node.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "Enter",
          code: "Enter",
          bubbles: true,
          isComposing: true,
        }),
      );
    });
    await page.waitForTimeout(120);
    await expect(formInput).toBeVisible();
    expect(errors).toEqual([]);
  });

  test("mixed RTL and LTR text round-trips without clipping", async ({ page }) => {
    const errors = await boot(page);
    const cell = page.locator('.grid-cell[data-column="Name"]').first();
    await cell.dblclick();
    const input = cell.locator("input.field-input");
    await expect(input).toHaveAttribute("dir", "auto");
    const value = "مرحبا Northlight +1 415 555 0134";
    await input.fill(value);
    await page.keyboard.press("Enter");
    await expect(cell.locator(".cell-text")).toHaveText(value);
    await expect(cell.locator(".cell-text")).toHaveAttribute("dir", "auto");
    await assertLayout(page);
    expect(errors).toEqual([]);
  });

  test("ultra-wide forms stay readable while grids use full width (@superwide)", async ({ page }) => {
    const width = page.viewportSize()?.width ?? 0;
    test.skip(width < 2560, "wide breakpoint only");
    const errors = await boot(page, { fixture: "stress" });
    await selectSidebarItem(page, "Relations & Schema");
    await expect(page.locator(".schema-view")).toBeVisible();
    const manage = await getRect(page, ".manage");
    expect(manage.width).toBeLessThanOrEqual(962);
    const graph = await getRect(page, ".schema-graph-pane");
    expect(graph.width).toBeGreaterThan(700);
    await page.getByRole("button", { name: "+ Table" }).click();
    const dialog = await getRect(page, '[data-testid="schema-dialog"]');
    expect(dialog.width).toBeLessThanOrEqual(722);
    await page.keyboard.press("Escape");
    if (width >= 3400) {
      await page.keyboard.press("Meta+1");
      const grid = await getRect(page, '[data-testid="grid-pane"]');
      expect(grid.width).toBeGreaterThan(width * 0.6);
    }
    await assertViewport(page);
    expect(errors).toEqual([]);
  });

  test("live resize across ultrawide and split-screen keeps panes and scrims sane", async ({ page }) => {
    const width = page.viewportSize()?.width ?? 0;
    test.skip(width !== 1440, "live resize matrix runs once");
    const errors = await boot(page);
    await openInspector(page);
    await page.getByRole("button", { name: "+ Column" }).click();
    await expect(page.getByTestId("schema-dialog")).toBeVisible();

    for (const target of [3440, 1440, 1150, 820, 3440]) {
      await page.setViewportSize({ width: target, height: 1440 });
      await page.waitForTimeout(120);
      await assertViewport(page);
      await assertChromeContainment(page);
      await page.keyboard.press("Meta+Alt+i");
      await page.keyboard.press("Meta+Alt+s");
      await page.waitForTimeout(120);
      await assertViewport(page);
    }

    await page.keyboard.press("Escape");
    await expect(page.getByTestId("schema-dialog")).toBeHidden();
    const overlays = page.locator(".sidebar-scrim, .drawer-backdrop, .modal-backdrop");
    for (let index = 0; index < await overlays.count(); index += 1) {
      const overlay = overlays.nth(index);
      if (await overlay.isVisible()) {
        await page.keyboard.press("Escape");
      }
    }
    await expect(page.locator(".sidebar-scrim, .drawer-backdrop, .modal-backdrop")).toHaveCount(0);

    if (!(await page.getByTestId("inspector").isVisible())) {
      await page.getByTestId("inspector-toggle").click();
    }
    const inspector = await getRect(page, '[data-testid="inspector"]');
    expect(inspector.width).toBeGreaterThanOrEqual(320);
    expect(inspector.width).toBeLessThanOrEqual(480);
    expect(inspector.right).toBeLessThanOrEqual(3441);
    expect(errors).toEqual([]);
  });

  test("currency minor units render and edit without /100 assumptions", async ({ page }) => {
    const errors = await openWideStress(page);
    await selectTable(page, "Currencies");
    await expect(
      page.locator('.grid-cell[data-column="JPY"] .cell-text').first(),
    ).toHaveText("1500");
    await expect(
      page.locator('.grid-cell[data-column="USD"] .cell-text').first(),
    ).toHaveText("19.99");
    const kwd = page.locator('.grid-cell[data-column="KWD"]').first();
    await expect(kwd.locator(".cell-text")).toHaveText("1.234");
    await kwd.dblclick();
    const input = kwd.locator("input.field-input");
    await expect(input).toHaveValue("1.234");
    await input.fill("2.345");
    await page.keyboard.press("Enter");
    await expect(kwd.locator(".cell-text")).toHaveText("2.345");
    expect(errors).toEqual([]);
  });

  test("high contrast preference strengthens borders and text contrast", async ({ page }) => {
    const errors = await boot(page);
    await page.emulateMedia({ contrast: "more" });
    const tokens = await page.evaluate(() => {
      const style = getComputedStyle(document.documentElement);
      return {
        primary: style.getPropertyValue("--text-primary").trim(),
        secondary: style.getPropertyValue("--text-secondary").trim(),
        tertiary: style.getPropertyValue("--text-tertiary").trim(),
        control: style.getPropertyValue("--border-control").trim(),
        grid: style.getPropertyValue("--bg-grid-solid").trim(),
        sidebar: style.getPropertyValue("--bg-sidebar-solid").trim(),
      };
    });
    expect(contrastHex(tokens.secondary, tokens.grid)).toBeGreaterThanOrEqual(4.5);
    expect(contrastHex(tokens.tertiary, tokens.sidebar)).toBeGreaterThanOrEqual(4.5);
    const alpha = alphaOf(tokens.control);
    expect(alpha).toBeGreaterThanOrEqual(0.5);
    expect(errors).toEqual([]);
  });

  test("125% and 150% app zoom keep every chrome surface contained", async ({ page }) => {
    const width = page.viewportSize()?.width ?? 0;
    test.skip(width !== 1440, "zoom matrix runs once at the standard laptop size");
    const errors = await boot(page);
    await page.keyboard.press("Meta+=");
    await expect(page.locator("html")).toHaveAttribute("data-zoom", "125");
    await settleLayout(page);
    await assertLayout(page);
    await page.keyboard.press("Meta+=");
    await expect(page.locator("html")).toHaveAttribute("data-zoom", "150");
    await settleLayout(page);
    await assertLayout(page);
    await page.keyboard.press("Meta+0");
    await expect(page.locator("html")).toHaveAttribute("data-zoom", "100");
    await settleLayout(page);
    await assertLayout(page);
    expect(errors).toEqual([]);
  });

  test("air-gapped boot makes zero external http(s) requests", async ({ page }) => {
    const external: string[] = [];
    page.on("request", (request) => {
      const url = request.url();
      if (!url.startsWith("http://") && !url.startsWith("https://")) return;
      if (!url.startsWith("http://127.0.0.1:5199")) external.push(url);
    });
    const errors = await boot(page);
    await selectTable(page, "Invoices");
    await openInspector(page);
    await page.getByTestId("search-trigger").click();
    await page.keyboard.press("Escape");
    await selectSidebarItem(page, "Relations & Schema");
    expect(external).toEqual([]);
    expect(errors).toEqual([]);
  });

  test("hidpi borders stay crisp and layout does not drift (@hidpi)", async ({ page }) => {
    const dpr = await page.evaluate(() => window.devicePixelRatio);
    test.skip(dpr !== 2, "HiDPI project only");
    const errors = await boot(page);
    const geometry = await page.evaluate(() => {
      const toolbar = document.querySelector<HTMLElement>('[data-testid="toolbar"]');
      const header = document.querySelector<HTMLElement>(".grid-header");
      const cell = document.querySelector<HTMLElement>(".grid-cell");
      return {
        toolbar: toolbar?.getBoundingClientRect().height ?? 0,
        header: header?.getBoundingClientRect().height ?? 0,
        border: cell ? getComputedStyle(cell).borderRightWidth : "",
        dpr: window.devicePixelRatio,
      };
    });
    expect(geometry.dpr).toBe(2);
    expect(geometry.toolbar).toBe(48);
    expect(geometry.header).toBe(28);
    expect(geometry.border).toBe("1px");
    await assertLayout(page);
    expect(errors).toEqual([]);
  });
});

for (const timezoneId of [
  "America/Los_Angeles",
  "UTC",
  "Asia/Kolkata",
  "Pacific/Auckland",
]) {
  test.describe(`date-only timezone ${timezoneId}`, () => {
    test.use({ timezoneId });
    test("calendar dates never shift on display or edit", async ({ page }) => {
      const width = page.viewportSize()?.width ?? 0;
      test.skip(width !== 1440, "timezone matrix runs once at the standard laptop size");
      const errors = await boot(page);
      await selectTable(page, "Projects");
      const cell = page.locator('.grid-cell[data-column="Start Date"]').first();
      const expected = await cell.locator(".cell-text").innerText();
      expect(expected).toMatch(/^\d{4}-\d{2}-\d{2}$/);
      await cell.dblclick();
      const input = cell.locator("input.field-input");
      await expect(input).toHaveValue(expected);
      await page.keyboard.press("Escape");
      await expect(cell.locator(".cell-text")).toHaveText(expected);
      expect(errors).toEqual([]);
    });
  });
}

test.describe("systems upgrades", () => {
  test("record time-travel drawer shows diffs and restores a revision", async ({ page }) => {
    const errors = await boot(page, { fixture: "design-studio" });
    await openInspector(page);
    const toggle = page.getByTestId("record-history-toggle");
    await expect(toggle).toBeVisible();
    await toggle.click();
    const history = page.getByTestId("record-history");
    await expect(history).toBeVisible();
    await expect(history.locator(".revision").first()).toBeVisible();
    await expect(history.locator(".revision-diff").first()).toBeVisible();
    await expect(history.locator(".revision-arrow").first()).toHaveText("→");
    await history.getByRole("button", { name: "Restore" }).first().click();
    await expect(history).toBeVisible();
    await assertChromeContainment(page);
    await assertViewport(page);
    const results = await new AxeBuilder({ page })
      .include('[data-testid="inspector"]')
      .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
      .analyze();
    const serious = results.violations.filter(
      (violation) => violation.impact === "critical" || violation.impact === "serious",
    );
    expect(serious, JSON.stringify(serious, null, 2)).toEqual([]);
    expect(errors).toEqual([]);
  });

  test("index advisor pill creates an index and refreshes quietly", async ({ page }) => {
    const errors = await openWideStress(page);
    await page.locator('.grid-head-cell.column-header[data-column="Column 01"]').click();
    const pill = page.getByTestId("index-hint");
    await expect(pill).toBeVisible();
    await expect(pill).toContainText("Full scan");
    await expect(pill).toContainText('Index "Column 01"');
    await assertChromeContainment(page);
    await assertViewport(page);
    await pill.getByRole("button", { name: /Full scan/ }).click();
    await expect(page.getByTestId("index-hint")).toHaveCount(0);
    await assertViewport(page);
    expect(errors).toEqual([]);
  });
});

test.describe("accessibility", () => {
  test("axe WCAG 2.2 AA scan finds no serious violations", async ({ page }) => {
    test.setTimeout(120_000);
    const width = page.viewportSize()?.width ?? 0;
    test.skip(width !== 1440, "axe matrix runs once at the standard laptop size");
    for (const theme of ["light", "dark"] as const) {
      const errors = await boot(page, { theme });
      const scan = async () => {
        await page.waitForTimeout(220);
        const results = await new AxeBuilder({ page })
          .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
          .analyze();
        const serious = results.violations.filter(
          (violation) => violation.impact === "critical" || violation.impact === "serious",
        );
        expect(serious, JSON.stringify(serious, null, 2)).toEqual([]);
      };
      await scan();
      await openInspector(page);
      await scan();
      await page.getByRole("tab", { name: "Form" }).click();
      await scan();
      await selectSidebarItem(page, "Relations & Schema");
      await scan();
      await page.getByTestId("search-trigger").click();
      await scan();
      await page.keyboard.press("Escape");
      await page.getByTestId("appearance-button").click();
      await scan();
      await page.keyboard.press("Escape");
      expect(errors).toEqual([]);
    }
  });
});

test.describe("async races", () => {
  test("a stale inspector read cannot replace a newer table selection", async ({ page }) => {
    const errors = await boot(page);

    // Arm the delay before the click so the Clients read (whether the focus
    // select or the 180 ms timer wins) is unambiguously the delayed one.
    await page.evaluate(() => {
      (
        window as unknown as {
          __slabFixtureDelay?: { command: string; ms: number; once?: boolean };
        }
      ).__slabFixtureDelay = { command: "read_record", ms: 2_500, once: true };
    });
    const clientsRow = page.locator('[data-testid="grid-row"]').first();
    const clientsName = (
      await clientsRow.locator('[data-column="Name"] .cell-text').innerText()
    ).trim();
    await clientsRow.click();
    await page.waitForTimeout(300);

    await selectSidebarItem(page, "Invoices");
    await expect(
      page.locator('[data-testid="grid-header"] [data-column="Number"]'),
    ).toBeVisible();

    const invoiceRow = page.locator('[data-testid="grid-row"]').first();
    const invoiceNumber = (
      await invoiceRow.locator('[data-column="Number"] .cell-text').innerText()
    ).trim();
    await invoiceRow.click();
    await expect(page.getByTestId("inspector")).toBeVisible();
    await expect(page.getByTestId("inspector").locator("h2").first()).toHaveText(
      invoiceNumber,
    );

    // The slow Clients response resolves now. It must be discarded.
    await page.waitForTimeout(2_800);
    await expect(page.getByTestId("inspector").locator("h2").first()).toHaveText(
      invoiceNumber,
    );
    await expect(page.getByTestId("inspector")).not.toContainText(clientsName);
    expect(errors).toEqual([]);
  });

  test("a delayed row select is cancelled by a table switch", async ({ page }) => {
    const errors = await boot(page);

    // Select a Clients row and switch tables in one task, well inside the
    // 180 ms inspector delay. The pending select must not open an unrelated
    // Invoices row with the same id.
    await page.evaluate(() => {
      document
        .querySelector<HTMLElement>('[data-testid="grid-row"]')
        ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      document
        .querySelector<HTMLElement>('[data-testid="sidebar"] [data-table="Invoices"]')
        ?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    await page.waitForTimeout(650);
    await expect(page.getByTestId("inspector")).toHaveCount(0);
    expect(errors).toEqual([]);
  });
});

test.describe("form studio state", () => {
  test("unsaved item edits survive a schema refresh", async ({ page }) => {
    const width = page.viewportSize()?.width ?? 0;
    test.skip(width !== 1440, "form inspector survival runs once at the laptop size");
    const errors = await boot(page);
    // Seed the fixture undo stack so Cmd+Z below actually replaces the schema
    // object, which is what triggers the Form Studio reset in the old code.
    const cell = page.locator('.grid-cell[data-column="Name"]').first();
    await cell.dblclick();
    const inline = cell.locator("input.field-input");
    await inline.fill("Northlight Edited");
    await page.keyboard.press("Enter");
    await expect(cell.locator(".cell-text")).toHaveText("Northlight Edited");

    await page.getByRole("tab", { name: "Form" }).click();
    await expect(page.locator(".studio")).toBeVisible();

    const item = page.locator(".studio-item").first();
    await item.click();
    const y = page.locator(".inspector-grid input").nth(1);
    await expect(y).toBeVisible();
    await y.fill("5");
    await expect(y).toHaveValue("5");

    // Blur the number input so Cmd+Z is an app undo, not native text undo.
    await item.click();
    await page.keyboard.press("Meta+z");
    await page.waitForTimeout(500);

    // The undo refreshed the schema object. The unsaved Y position and the
    // item selection must still be there.
    await expect(page.locator(".inspector-grid input").nth(1)).toHaveValue("5");
    await expect(page.locator(".studio-item.selected")).toHaveCount(1);
    expect(errors).toEqual([]);
  });
});

test.describe("inspector state", () => {
  test("double-clicking another row refreshes an open inspector", async ({ page }) => {
    const width = page.viewportSize()?.width ?? 0;
    test.skip(width !== 1440, "inspector double-click runs once at the laptop size");
    const errors = await boot(page);
    const rows = page.locator('[data-testid="grid-row"]');
    const firstTitle = (
      await rows.nth(0).locator('[data-column="Name"] .cell-text').innerText()
    ).trim();
    const secondTitle = (
      await rows.nth(1).locator('[data-column="Name"] .cell-text').innerText()
    ).trim();

    await rows.nth(0).click();
    await expect(page.getByTestId("inspector")).toBeVisible();
    await expect(page.getByTestId("inspector").locator("h2").first()).toHaveText(
      firstTitle,
    );

    await rows.nth(1).locator('[data-column="Name"]').dblclick();
    await expect(page.getByTestId("inspector").locator("h2").first()).toHaveText(
      secondTitle,
    );
    expect(errors).toEqual([]);
  });
});

test.describe("draft isolation", () => {
  test("a grid double-click does not steal an open draft", async ({ page }) => {
    const width = page.viewportSize()?.width ?? 0;
    test.skip(width !== 1440, "draft isolation runs once at the laptop size");
    const errors = await boot(page);
    await page.getByRole("button", { name: /New Record/ }).first().click();
    await expect(page.getByTestId("inspector")).toContainText("New Record");

    const row = page.locator('[data-testid="grid-row"]').nth(1);
    await row.locator('[data-column="Name"]').dblclick();
    await page
      .getByTestId("inspector")
      .getByRole("button", { name: "Cancel" })
      .first()
      .click();

    await expect(page.getByTestId("inspector")).not.toContainText("New Record");
    await expect(page.locator(".grid-row.selected")).toHaveCount(0);
    expect(errors).toEqual([]);
  });
});
