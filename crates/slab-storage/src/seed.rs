//! The design studio sample workspace used by the first-run screen and fixtures.

use slab_core::{
    ColumnDraft, ColumnType, CurrencyCode, Decimal, DeleteAction, GridSpec, Layout, LayoutItem,
    LayoutItemKind, Money, RelationDraft, RowData, SlabDate, TableDraft, Value,
};

use crate::error::StorageError;
use crate::workspace::Workspace;

fn text(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Text,
        required: false,
        unique: false,
        default: None,
    }
}

fn required_text(name: &str) -> ColumnDraft {
    ColumnDraft {
        required: true,
        ..text(name)
    }
}

fn integer(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Integer,
        required: false,
        unique: false,
        default: None,
    }
}

fn required_integer(name: &str) -> ColumnDraft {
    ColumnDraft {
        required: true,
        ..integer(name)
    }
}

fn date(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Date,
        required: false,
        unique: false,
        default: None,
    }
}

fn currency(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Currency {
            currency: CurrencyCode::from_static(*b"USD"),
        },
        required: false,
        unique: false,
        default: None,
    }
}

fn decimal(name: &str, scale: u8, default: Option<&str>) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Decimal { scale },
        required: false,
        unique: false,
        default: default.and_then(|value| Decimal::parse(value).ok().map(Value::Decimal)),
    }
}

fn select(name: &str, options: &[&str], default: Option<&str>) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Select {
            options: options.iter().map(|option| (*option).to_owned()).collect(),
        },
        required: false,
        unique: false,
        default: default.map(|option| Value::Select(option.to_owned())),
    }
}

fn formula(name: &str, expression: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.into(),
        ty: ColumnType::Formula {
            expression: expression.into(),
        },
        required: false,
        unique: false,
        default: None,
    }
}

fn row(entries: &[(&str, Value)]) -> RowData {
    entries
        .iter()
        .map(|(name, value)| ((*name).to_owned(), value.clone()))
        .collect()
}

fn money(amount: &str) -> Result<Value, StorageError> {
    Ok(Value::Currency(Money::from_decimal(
        CurrencyCode::from_static(*b"USD"),
        Decimal::parse(amount)?,
    )?))
}

fn date_value(value: &str) -> Result<Value, StorageError> {
    Ok(Value::Date(SlabDate::parse(value)?))
}

impl Workspace {
    /// Loads the sample design studio workspace. Requires an empty workspace.
    pub fn seed_studio(&mut self) -> Result<(), StorageError> {
        let schema = self.schema()?;
        if !schema.tables.is_empty() {
            return Err(StorageError::Command(
                "the sample can only be loaded into an empty workspace".into(),
            ));
        }

        self.create_table(&TableDraft {
            name: "Clients".into(),
            columns: vec![
                required_text("Name"),
                text("Email"),
                text("Phone"),
                text("City"),
                text("Notes"),
            ],
        })?;
        self.create_table(&TableDraft {
            name: "Projects".into(),
            columns: vec![
                required_text("Name"),
                required_integer("Client"),
                select(
                    "Status",
                    &["Planning", "Active", "Review", "Complete"],
                    Some("Planning"),
                ),
                currency("Budget"),
                date("Start Date"),
                date("Due Date"),
                text("Brief"),
            ],
        })?;
        self.create_table(&TableDraft {
            name: "Invoices".into(),
            columns: vec![
                ColumnDraft {
                    unique: true,
                    ..required_text("Number")
                },
                required_integer("Client"),
                required_integer("Project"),
                date("Issued"),
                date("Due"),
                select("Status", &["Draft", "Sent", "Paid", "Void"], Some("Draft")),
                decimal("Tax Rate", 4, Some("0.0825")),
                text("Note"),
            ],
        })?;
        self.create_table(&TableDraft {
            name: "Line Items".into(),
            columns: vec![
                required_text("Description"),
                decimal("Quantity", 2, None),
                currency("Unit Price"),
                required_integer("Invoice"),
                formula("Amount", "ROUND([Quantity] * [Unit Price], 2)"),
            ],
        })?;

        self.create_relation(&RelationDraft {
            name: "Client Projects".into(),
            parent_table: "Clients".into(),
            child_table: "Projects".into(),
            child_column: "Client".into(),
            on_delete: DeleteAction::Cascade,
        })?;
        self.create_relation(&RelationDraft {
            name: "Project Invoices".into(),
            parent_table: "Projects".into(),
            child_table: "Invoices".into(),
            child_column: "Project".into(),
            on_delete: DeleteAction::Cascade,
        })?;
        self.create_relation(&RelationDraft {
            name: "Client Invoices".into(),
            parent_table: "Clients".into(),
            child_table: "Invoices".into(),
            child_column: "Client".into(),
            on_delete: DeleteAction::Restrict,
        })?;
        self.create_relation(&RelationDraft {
            name: "Invoice Lines".into(),
            parent_table: "Invoices".into(),
            child_table: "Line Items".into(),
            child_column: "Invoice".into(),
            on_delete: DeleteAction::Cascade,
        })?;

        self.add_column(
            "Invoices",
            &formula("Subtotal", "SUM([Invoice Lines].[Amount])"),
        )?;
        self.add_column(
            "Invoices",
            &formula("Tax", "ROUND([Subtotal] * [Tax Rate], 2)"),
        )?;
        self.add_column("Invoices", &formula("Total", "[Subtotal] + [Tax]"))?;

        let northlight = self.insert_row(
            "Clients",
            &row(&[
                ("Name", Value::Text("Northlight Studio".into())),
                ("Email", Value::Text("studio@northlight.example".into())),
                ("Phone", Value::Text("+1 415 555 0144".into())),
                ("City", Value::Text("San Francisco".into())),
                (
                    "Notes",
                    Value::Text("Brand system and packaging work. Net 30.".into()),
                ),
            ]),
        )?;
        let hiraya = self.insert_row(
            "Clients",
            &row(&[
                ("Name", Value::Text("Hiraya Ceramics".into())),
                ("Email", Value::Text("hello@hiraya.example".into())),
                ("City", Value::Text("Portland".into())),
                (
                    "Notes",
                    Value::Text("Seasonal catalogue, photography on site.".into()),
                ),
            ]),
        )?;
        let fieldnote = self.insert_row(
            "Clients",
            &row(&[
                ("Name", Value::Text("Fieldnote Labs".into())),
                ("Email", Value::Text("ops@fieldnote.example".into())),
                ("City", Value::Text("Austin".into())),
            ]),
        )?;

        let identity = self.insert_row(
            "Projects",
            &row(&[
                ("Name", Value::Text("Identity refresh".into())),
                ("Client", Value::Integer(northlight.id)),
                ("Status", Value::Select("Active".into())),
                ("Budget", money("24000.00")?),
                ("Start Date", date_value("2026-09-01")?),
                ("Due Date", date_value("2026-12-15")?),
            ]),
        )?;
        let packaging = self.insert_row(
            "Projects",
            &row(&[
                ("Name", Value::Text("Packaging system".into())),
                ("Client", Value::Integer(northlight.id)),
                ("Status", Value::Select("Review".into())),
                ("Budget", money("18500.00")?),
                ("Start Date", date_value("2026-06-10")?),
                ("Due Date", date_value("2026-11-30")?),
            ]),
        )?;
        let catalogue = self.insert_row(
            "Projects",
            &row(&[
                ("Name", Value::Text("Autumn catalogue".into())),
                ("Client", Value::Integer(hiraya.id)),
                ("Status", Value::Select("Planning".into())),
                ("Budget", money("9200.00")?),
                ("Start Date", date_value("2026-11-01")?),
                ("Due Date", date_value("2027-02-20")?),
            ]),
        )?;
        let research = self.insert_row(
            "Projects",
            &row(&[
                ("Name", Value::Text("Field study".into())),
                ("Client", Value::Integer(fieldnote.id)),
                ("Status", Value::Select("Complete".into())),
                ("Budget", money("6000.00")?),
                ("Start Date", date_value("2026-03-01")?),
                ("Due Date", date_value("2026-05-15")?),
            ]),
        )?;

        let invoice_a = self.insert_row(
            "Invoices",
            &row(&[
                ("Number", Value::Text("INV-2026-001".into())),
                ("Client", Value::Integer(northlight.id)),
                ("Project", Value::Integer(identity.id)),
                ("Issued", date_value("2026-09-15")?),
                ("Due", date_value("2026-10-15")?),
                ("Status", Value::Select("Paid".into())),
                ("Tax Rate", Value::Decimal(Decimal::parse("0.0825")?)),
            ]),
        )?;
        let invoice_b = self.insert_row(
            "Invoices",
            &row(&[
                ("Number", Value::Text("INV-2026-002".into())),
                ("Client", Value::Integer(northlight.id)),
                ("Project", Value::Integer(packaging.id)),
                ("Issued", date_value("2026-10-02")?),
                ("Due", date_value("2026-11-01")?),
                ("Status", Value::Select("Sent".into())),
                ("Tax Rate", Value::Decimal(Decimal::parse("0.0825")?)),
            ]),
        )?;
        let invoice_c = self.insert_row(
            "Invoices",
            &row(&[
                ("Number", Value::Text("INV-2026-003".into())),
                ("Client", Value::Integer(hiraya.id)),
                ("Project", Value::Integer(catalogue.id)),
                ("Issued", date_value("2026-11-12")?),
                ("Due", date_value("2026-12-12")?),
                ("Status", Value::Select("Draft".into())),
                ("Tax Rate", Value::Decimal(Decimal::parse("0.0000")?)),
            ]),
        )?;
        let invoice_d = self.insert_row(
            "Invoices",
            &row(&[
                ("Number", Value::Text("INV-2026-004".into())),
                ("Client", Value::Integer(fieldnote.id)),
                ("Project", Value::Integer(research.id)),
                ("Issued", date_value("2026-05-20")?),
                ("Due", date_value("2026-06-19")?),
                ("Status", Value::Select("Paid".into())),
                ("Tax Rate", Value::Decimal(Decimal::parse("0.0825")?)),
            ]),
        )?;

        let lines = [
            (invoice_a.id, "Discovery workshop", "2.00", "1400.00"),
            (invoice_a.id, "Identity directions", "3.00", "3200.00"),
            (invoice_a.id, "Design system build", "1.00", "7800.00"),
            (invoice_b.id, "Structural design", "4.00", "1450.00"),
            (invoice_b.id, "Print production files", "1.00", "2600.00"),
            (invoice_b.id, "Proofing", "6.00", "180.00"),
            (invoice_c.id, "Art direction", "3.00", "1250.00"),
            (invoice_c.id, "Retouching", "12.00", "95.00"),
            (invoice_d.id, "Field interviews", "8.00", "320.00"),
            (invoice_d.id, "Synthesis report", "1.00", "2400.00"),
        ];
        for (invoice, description, quantity, unit_price) in lines {
            self.insert_row(
                "Line Items",
                &row(&[
                    ("Description", Value::Text(description.into())),
                    ("Quantity", Value::Decimal(Decimal::parse(quantity)?)),
                    ("Unit Price", money(unit_price)?),
                    ("Invoice", Value::Integer(invoice)),
                ]),
            )?;
        }

        self.save_layout(client_layout())?;
        self.save_layout(project_layout())?;
        self.save_layout(invoice_layout())?;
        self.clear_history();
        Ok(())
    }
}

fn field(id: &str, column: &str, x: u16, y: u16, w: u16) -> LayoutItem {
    LayoutItem {
        id: id.into(),
        kind: LayoutItemKind::Field {
            column: column.into(),
        },
        x,
        y,
        w,
        h: 2,
    }
}

fn section(id: &str, title: &str, y: u16) -> LayoutItem {
    LayoutItem {
        id: id.into(),
        kind: LayoutItemKind::Section {
            title: title.into(),
        },
        x: 0,
        y,
        w: 12,
        h: 1,
    }
}

fn client_layout() -> Layout {
    Layout {
        id: "layout_client".into(),
        name: "Client Record".into(),
        table: "Clients".into(),
        grid: GridSpec::default(),
        items: vec![
            section("client_s1", "Contact", 0),
            field("client_name", "Name", 0, 1, 6),
            field("client_email", "Email", 6, 1, 6),
            field("client_phone", "Phone", 0, 4, 4),
            field("client_city", "City", 4, 4, 4),
            section("client_s2", "Notes", 7),
            LayoutItem {
                id: "client_notes".into(),
                kind: LayoutItemKind::Field {
                    column: "Notes".into(),
                },
                x: 0,
                y: 8,
                w: 12,
                h: 5,
            },
            LayoutItem {
                id: "client_projects".into(),
                kind: LayoutItemKind::Portal {
                    relation: "Client Projects".into(),
                    columns: vec!["Name".into(), "Status".into(), "Due Date".into()],
                    rows: 5,
                },
                x: 0,
                y: 14,
                w: 12,
                h: 6,
            },
        ],
        updated_at: 0,
    }
}

fn project_layout() -> Layout {
    Layout {
        id: "layout_project".into(),
        name: "Project Record".into(),
        table: "Projects".into(),
        grid: GridSpec::default(),
        items: vec![
            section("project_s1", "Project", 0),
            field("project_name", "Name", 0, 1, 8),
            field("project_status", "Status", 8, 1, 4),
            field("project_budget", "Budget", 0, 4, 4),
            field("project_start", "Start Date", 4, 4, 4),
            field("project_due", "Due Date", 8, 4, 4),
            section("project_s2", "Brief", 7),
            LayoutItem {
                id: "project_brief".into(),
                kind: LayoutItemKind::Field {
                    column: "Brief".into(),
                },
                x: 0,
                y: 8,
                w: 12,
                h: 4,
            },
        ],
        updated_at: 0,
    }
}

fn invoice_layout() -> Layout {
    Layout {
        id: "layout_invoice".into(),
        name: "Invoice".into(),
        table: "Invoices".into(),
        grid: GridSpec::default(),
        items: vec![
            section("invoice_s1", "Invoice", 0),
            field("invoice_number", "Number", 0, 1, 4),
            field("invoice_status", "Status", 4, 1, 4),
            field("invoice_issued", "Issued", 8, 1, 4),
            field("invoice_client", "Client", 0, 4, 4),
            field("invoice_project", "Project", 4, 4, 4),
            field("invoice_due", "Due", 8, 4, 4),
            field("invoice_tax_rate", "Tax Rate", 0, 7, 4),
            field("invoice_subtotal", "Subtotal", 4, 7, 4),
            field("invoice_tax", "Tax", 8, 7, 4),
            LayoutItem {
                id: "invoice_total".into(),
                kind: LayoutItemKind::Field {
                    column: "Total".into(),
                },
                x: 8,
                y: 10,
                w: 4,
                h: 2,
            },
            LayoutItem {
                id: "invoice_lines".into(),
                kind: LayoutItemKind::Portal {
                    relation: "Invoice Lines".into(),
                    columns: vec![
                        "Description".into(),
                        "Quantity".into(),
                        "Unit Price".into(),
                        "Amount".into(),
                    ],
                    rows: 6,
                },
                x: 0,
                y: 13,
                w: 12,
                h: 8,
            },
        ],
        updated_at: 0,
    }
}

/// A helper for tests that checks the seed produced a coherent invoice.
#[cfg(test)]
pub(crate) fn expected_invoice_totals() -> (i64, i64, i64) {
    // Three lines in INV-2026-001: 2800 + 9600 + 7800 = 20200; tax 1666.50; total 21866.50.
    (2_020_000, 166_650, 2_186_650)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestClock;

    #[test]
    fn sample_workspace_has_rollups() {
        let mut workspace =
            Workspace::open_in_memory("Sample", Box::new(TestClock::new())).expect("workspace");
        workspace.seed_studio().expect("seed");
        let page = workspace
            .page(&slab_core::PageQuery::first("Invoices", 10))
            .expect("page");
        let invoice = page.rows.first().expect("invoice");
        let subtotal = invoice.get("Subtotal").cloned().unwrap_or(Value::Null);
        let total = invoice.get("Total").cloned().unwrap_or(Value::Null);
        assert!(matches!(subtotal, Value::Currency(_)));
        assert!(matches!(total, Value::Currency(_)));
        let (expected_subtotal, _, expected_total) = expected_invoice_totals();
        assert_eq!(
            subtotal,
            Value::Currency(
                Money::from_minor(CurrencyCode::parse("USD").expect("usd"), expected_subtotal)
                    .expect("money")
            )
        );
        assert_eq!(
            total,
            Value::Currency(
                Money::from_minor(CurrencyCode::parse("USD").expect("usd"), expected_total)
                    .expect("money")
            )
        );
    }
}
