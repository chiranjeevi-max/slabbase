import { expect, test } from "@playwright/test";
import type {
  Filter,
  IndexSpec,
  Layout,
  PageFrame,
  PageQuery,
  Record as SlabRecord,
  Schema,
  UndoState,
  WorkspaceInfoDto,
} from "../src/bindings";
import { fixtureCall, resetFixtureForTests, type FixtureMode } from "../src/fixture";

test.beforeEach(() => resetFixtureForTests());

test("the stress fixture reports its own workspace metadata", async () => {
  const info = await fixtureCall<WorkspaceInfoDto>("workspace_info", {}, "stress");
  expect(info).toMatchObject({
    name: "Stress",
    path: null,
    tables: [{ name: "Wide", rowCount: 120 }, { name: "Currencies", rowCount: 1 }],
  });
});

async function filteredPage(table: string, filters: Filter[], overrides: Partial<PageQuery> = {}) {
  const query: PageQuery = {
    table, filters, cursor: null, sort: [], search: null, limit: 50, ...overrides,
  };
  return fixtureCall<PageFrame>("page_rows", { query }, "full");
}

test("fixture filters intersect search before applying the page limit", async () => {
  for (const id of [1, 2, 3]) {
    await fixtureCall("update_row", {
      table: "Projects", id, values: { Brief: { Text: "Matching brief" } },
    }, "full");
  }
  const page = await filteredPage("Projects", [
    { column: "Budget", op: "lte", value: { Text: "USD 18500.00" } },
    { column: "Name", op: "contains", value: { Text: "I" } },
  ], { search: "matching brief", limit: 1 });
  expect(page.rows.map((row) => row.id)).toEqual([2]);
  expect(page.total_rows).toBe(1);

  const pageWithoutSearch = await filteredPage("Projects", [
    { column: "Budget", op: "startsWith", value: { Text: "18500" } },
    { column: "Name", op: "endsWith", value: { Text: "SYSTEM" } },
  ], { limit: 1 });
  expect(pageWithoutSearch.rows.map((row) => row.id)).toEqual([2]);
});

test("fixture filters distinguish nulls from displayed and stored values", async () => {
  const textMatch = await filteredPage("Clients", [
    { column: "Notes", op: "contains", value: { Text: "" } },
  ]);
  expect(textMatch.rows.map((row) => row.id)).toEqual([1, 2]);

  const explicitNull = await filteredPage("Clients", [{ column: "Notes", op: "eq", value: "Null" }]);
  expect(explicitNull.rows.map((row) => row.id)).toEqual([3]);

  const isNull = await filteredPage("Clients", [{ column: "Notes", op: "isNull", value: null }]);
  expect(isNull.rows.map((row) => row.id)).toEqual([3]);

  const notEqual = await filteredPage("Clients", [{ column: "Notes", op: "notEq", value: { Text: "" } }]);
  expect(notEqual.rows.map((row) => row.id)).toEqual([1, 2, 3]);

  const range = await filteredPage("Projects", [{ column: "Budget", op: "lt", value: { Currency: { currency: "USD", amount: "10000" } } }]);
  expect(range.rows.map((row) => row.id)).toEqual([3, 4]);

  await expect(filteredPage("Projects", [
    { column: "Budget", op: "eq", value: { Text: "18500.001" } },
  ])).rejects.toMatchObject({ code: "SLAB_E_QUERY" });
});

const schema = (mode: FixtureMode = "full") => fixtureCall<Schema>("get_schema", {}, mode);

test("reinitializing the fixture restores original nested schema data", async () => {
  await fixtureCall("rename_column", { table: "Invoices", old_name: "Tax Rate", new_name: "Tax Percentage" }, "full");
  const edited = await schema();
  const editedTax = edited.tables.find((table) => table.name === "Invoices")?.columns.find((column) => column.name === "Tax");
  expect(editedTax?.ty).toMatchObject({ expression: "ROUND([Subtotal] * [Tax Percentage], 2)" });

  resetFixtureForTests();
  const reopened = await schema();
  const tax = reopened.tables.find((table) => table.name === "Invoices")?.columns.find((column) => column.name === "Tax");
  expect(tax?.ty).toMatchObject({ expression: "ROUND([Subtotal] * [Tax Rate], 2)" });
  const field = reopened.layouts.find((layout) => layout.id === "layout_invoice")?.items.find((item) => item.id === "invoice_tax_rate");
  expect(field?.kind).toEqual({ kind: "field", column: "Tax Rate" });
});

test("an empty fixture can create its first table and load a fresh sample", async () => {
  await expect(schema("empty")).resolves.toEqual({ tables: [], relations: [], layouts: [] });
  await fixtureCall("create_table", { draft: { name: "Clients", columns: [] } }, "empty");
  const created = await fixtureCall<WorkspaceInfoDto>("workspace_info", {}, "empty");
  expect(created.tables).toEqual([{ name: "Clients", rowCount: 0 }]);
  await fixtureCall("undo", {}, "empty");
  expect((await schema("empty")).tables).toEqual([]);
  await fixtureCall("redo", {}, "empty");

  const sample = await fixtureCall<WorkspaceInfoDto>("load_sample", {}, "empty");
  expect(sample.name).toBe("Design Studio");
  expect(sample.tables).toContainEqual({ name: "Clients", rowCount: 3 });
  expect(sample.tables.map((table) => table.name)).toEqual(["Clients", "Projects", "Invoices", "Line Items"]);
});

test("listing and toggling indexes return the table's current indexes", async () => {
  const toggle = (column: string, enabled: boolean) => fixtureCall<IndexSpec[]>("toggle_index", { table: "Projects", column, enabled }, "full");
  const list = (table: string) => fixtureCall<IndexSpec[]>("list_indexes", { table }, "full");
  const dueIndex: IndexSpec = { name: "slab_idx_Projects_Due_Date", table: "Projects", columns: ["Due Date"], unique: false };
  expect(await toggle("Due Date", true)).toEqual([dueIndex]);
  expect(await toggle("Due Date", true)).toEqual([dueIndex]);
  expect(await list("Projects")).toEqual([dueIndex]);
  expect(await list("Clients")).toEqual([]);

  const secondIndex = await fixtureCall<IndexSpec>("create_index", { table: "Projects", column: "Budget" }, "full");
  expect(await toggle("Due Date", false)).toEqual([secondIndex]);
  expect(await list("Projects")).toEqual([secondIndex]);
});

test("adding a stored column materializes its default in existing rows", async () => {
  const draft = { name: "Approved", ty: { kind: "boolean" }, required: false, unique: false, default: { Boolean: true } };
  await fixtureCall("add_column", { table: "Clients", draft }, "full");
  const client = await fixtureCall<SlabRecord>("read_record", { table: "Clients", id: 2 }, "full");
  expect(client.values["Approved"]).toEqual({ Boolean: true });

  await fixtureCall("undo", {}, "full");
  expect((await fixtureCall<SlabRecord>("read_record", { table: "Clients", id: 2 }, "full")).values).not.toHaveProperty("Approved");
});

test("fixture boolean filters use Yes and No without matching nulls", async () => {
  const draft = { name: "Approved", ty: { kind: "boolean" }, required: false, unique: false, default: null };
  await fixtureCall("add_column", { table: "Clients", draft }, "full");
  await fixtureCall("update_row", { table: "Clients", id: 1, values: { Approved: { Boolean: false } } }, "full");
  await fixtureCall("update_row", { table: "Clients", id: 2, values: { Approved: { Boolean: true } } }, "full");
  const yes = await filteredPage("Clients", [{ column: "Approved", op: "contains", value: { Boolean: true } }]);
  expect(yes.rows.map((row) => row.id)).toEqual([2]);
  const no = await filteredPage("Clients", [{ column: "Approved", op: "eq", value: { Text: "No" } }]);
  expect(no.rows.map((row) => row.id)).toEqual([1]);
});

test("dropping a child column displayed in a parent form is rejected", async () => {
  await expect(fixtureCall("drop_column", { table: "Line Items", column: "Description" }, "full"))
    .rejects.toMatchObject({ code: "SLAB_E_FORMULA_REF", message: /used by form Invoice/ });
  expect((await schema()).tables.find((table) => table.name === "Line Items")?.columns.map((column) => column.name)).toContain("Description");
});

test("renaming a child column follows the portal through undo and redo", async () => {
  const portalColumns = async () => {
    const portal = (await schema()).layouts.find((layout) => layout.id === "layout_invoice")?.items.find((item) => item.id === "invoice_lines");
    return portal?.kind.kind === "portal" ? portal.kind.columns : [];
  };
  await fixtureCall("rename_column", { table: "Line Items", old_name: "Description", new_name: "Detail" }, "full");
  expect(await portalColumns()).toEqual(["Detail", "Quantity", "Unit Price", "Amount"]);
  await fixtureCall("undo", {}, "full");
  expect(await portalColumns()).toEqual(["Description", "Quantity", "Unit Price", "Amount"]);
  await fixtureCall("redo", {}, "full");
  expect(await portalColumns()).toEqual(["Detail", "Quantity", "Unit Price", "Amount"]);
});

test("deleting a missing fixture layout returns not found", async () => {
  await expect(fixtureCall("delete_layout", { id: "missing" }, "full")).rejects.toMatchObject({ code: "SLAB_E_NOT_FOUND" });
});

test("deleting a fixture layout records the inverse", async () => {
  const id = "layout_client";
  const original = (await schema()).layouts.find((layout) => layout.id === id);
  expect(original).toBeDefined();
  await fixtureCall("delete_layout", { id }, "full");
  expect((await schema()).layouts.some((layout) => layout.id === id)).toBe(false);
  const undo = await fixtureCall<UndoState>("undo_state", {}, "full");
  expect(undo.undo_label).toBe("Delete form Client Record");
  await fixtureCall("undo", {}, "full");
  expect((await schema()).layouts.find((layout) => layout.id === id)).toEqual(original);
  await fixtureCall("redo", {}, "full");
  expect((await schema()).layouts.some((layout) => layout.id === id)).toBe(false);
});

test("new fixture layouts get distinct IDs and can be edited by ID", async () => {
  const draft: Layout = { id: "", name: "First Form", table: "Clients", grid: { columns: 12, row_height: 28 }, items: [], updated_at: 0 };
  const save = (layout: Layout) => fixtureCall<Layout>("save_layout", { layout }, "full");
  const first = await save(draft);
  const second = await save({ ...draft, name: "Second Form" });
  expect(first.id).not.toBe("");
  expect(second.id).not.toBe("");
  expect(second.id).not.toBe(first.id);
  await save({ ...first, name: "Renamed Form" });
  const layouts = (await schema()).layouts;
  expect(layouts.filter((layout) => layout.id === first.id).map((layout) => layout.name)).toEqual(["Renamed Form"]);
  expect(layouts.find((layout) => layout.id === second.id)?.name).toBe("Second Form");
});
