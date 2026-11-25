// Generated from fixtures/design-studio.slab. Do not edit by hand.
// Regenerate with the repository fixture when the sample workspace changes.
import type {
  AuditEntry,
  Record as SlabRecord,
  Schema,
  WorkspaceInfoDto,
} from "../bindings";

export const DESIGN_STUDIO_INFO: WorkspaceInfoDto = {
  "name": "Design Studio",
  "path": null,
  "formatVersion": 1,
  "journalMode": "wal",
  "tables": [
    {
      "name": "Clients",
      "rowCount": 3
    },
    {
      "name": "Projects",
      "rowCount": 4
    },
    {
      "name": "Invoices",
      "rowCount": 4
    },
    {
      "name": "Line Items",
      "rowCount": 10
    }
  ]
};

export const DESIGN_STUDIO_SCHEMA: Schema = {
  "tables": [
    {
      "id": "t_1a20fa93046_1",
      "name": "Clients",
      "columns": [
        {
          "id": "c_1a20fa93046_2",
          "table": "Clients",
          "name": "Name",
          "ty": {
            "kind": "text"
          },
          "required": true,
          "unique": false,
          "default": null,
          "position": 0
        },
        {
          "id": "c_1a20fa93046_3",
          "table": "Clients",
          "name": "Email",
          "ty": {
            "kind": "text"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 1
        },
        {
          "id": "c_1a20fa93046_4",
          "table": "Clients",
          "name": "Phone",
          "ty": {
            "kind": "text"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 2
        },
        {
          "id": "c_1a20fa93046_5",
          "table": "Clients",
          "name": "City",
          "ty": {
            "kind": "text"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 3
        },
        {
          "id": "c_1a20fa93046_6",
          "table": "Clients",
          "name": "Notes",
          "ty": {
            "kind": "text"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 4
        }
      ],
      "created_at": 1795559075910
    },
    {
      "id": "t_1a20fa93047_7",
      "name": "Projects",
      "columns": [
        {
          "id": "c_1a20fa93047_8",
          "table": "Projects",
          "name": "Name",
          "ty": {
            "kind": "text"
          },
          "required": true,
          "unique": false,
          "default": null,
          "position": 0
        },
        {
          "id": "c_1a20fa93047_9",
          "table": "Projects",
          "name": "Client",
          "ty": {
            "kind": "integer"
          },
          "required": true,
          "unique": false,
          "default": null,
          "position": 1
        },
        {
          "id": "c_1a20fa93047_a",
          "table": "Projects",
          "name": "Status",
          "ty": {
            "kind": "select",
            "options": [
              "Planning",
              "Active",
              "Review",
              "Complete"
            ]
          },
          "required": false,
          "unique": false,
          "default": {
            "Select": "Planning"
          },
          "position": 2
        },
        {
          "id": "c_1a20fa93047_b",
          "table": "Projects",
          "name": "Budget",
          "ty": {
            "kind": "currency",
            "currency": "USD"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 3
        },
        {
          "id": "c_1a20fa93047_c",
          "table": "Projects",
          "name": "Start Date",
          "ty": {
            "kind": "date"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 4
        },
        {
          "id": "c_1a20fa93047_d",
          "table": "Projects",
          "name": "Due Date",
          "ty": {
            "kind": "date"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 5
        },
        {
          "id": "c_1a20fa93047_e",
          "table": "Projects",
          "name": "Brief",
          "ty": {
            "kind": "text"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 6
        }
      ],
      "created_at": 1795559075911
    },
    {
      "id": "t_1a20fa93047_f",
      "name": "Invoices",
      "columns": [
        {
          "id": "c_1a20fa93047_10",
          "table": "Invoices",
          "name": "Number",
          "ty": {
            "kind": "text"
          },
          "required": true,
          "unique": true,
          "default": null,
          "position": 0
        },
        {
          "id": "c_1a20fa93047_11",
          "table": "Invoices",
          "name": "Client",
          "ty": {
            "kind": "integer"
          },
          "required": true,
          "unique": false,
          "default": null,
          "position": 1
        },
        {
          "id": "c_1a20fa93047_12",
          "table": "Invoices",
          "name": "Project",
          "ty": {
            "kind": "integer"
          },
          "required": true,
          "unique": false,
          "default": null,
          "position": 2
        },
        {
          "id": "c_1a20fa93047_13",
          "table": "Invoices",
          "name": "Issued",
          "ty": {
            "kind": "date"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 3
        },
        {
          "id": "c_1a20fa93047_14",
          "table": "Invoices",
          "name": "Due",
          "ty": {
            "kind": "date"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 4
        },
        {
          "id": "c_1a20fa93047_15",
          "table": "Invoices",
          "name": "Status",
          "ty": {
            "kind": "select",
            "options": [
              "Draft",
              "Sent",
              "Paid",
              "Void"
            ]
          },
          "required": false,
          "unique": false,
          "default": {
            "Select": "Draft"
          },
          "position": 5
        },
        {
          "id": "c_1a20fa93047_16",
          "table": "Invoices",
          "name": "Tax Rate",
          "ty": {
            "kind": "decimal",
            "scale": 4
          },
          "required": false,
          "unique": false,
          "default": {
            "Decimal": "0.0825"
          },
          "position": 6
        },
        {
          "id": "c_1a20fa93047_17",
          "table": "Invoices",
          "name": "Note",
          "ty": {
            "kind": "text"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 7
        },
        {
          "id": "c_1a20fa9304d_22",
          "table": "Invoices",
          "name": "Subtotal",
          "ty": {
            "kind": "formula",
            "expression": "SUM([Invoice Lines].[Amount])"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 8
        },
        {
          "id": "c_1a20fa9304d_23",
          "table": "Invoices",
          "name": "Tax",
          "ty": {
            "kind": "formula",
            "expression": "ROUND([Subtotal] * [Tax Rate], 2)"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 9
        },
        {
          "id": "c_1a20fa9304d_24",
          "table": "Invoices",
          "name": "Total",
          "ty": {
            "kind": "formula",
            "expression": "[Subtotal] + [Tax]"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 10
        }
      ],
      "created_at": 1795559075911
    },
    {
      "id": "t_1a20fa93048_18",
      "name": "Line Items",
      "columns": [
        {
          "id": "c_1a20fa93048_19",
          "table": "Line Items",
          "name": "Description",
          "ty": {
            "kind": "text"
          },
          "required": true,
          "unique": false,
          "default": null,
          "position": 0
        },
        {
          "id": "c_1a20fa93048_1a",
          "table": "Line Items",
          "name": "Quantity",
          "ty": {
            "kind": "decimal",
            "scale": 2
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 1
        },
        {
          "id": "c_1a20fa93048_1b",
          "table": "Line Items",
          "name": "Unit Price",
          "ty": {
            "kind": "currency",
            "currency": "USD"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 2
        },
        {
          "id": "c_1a20fa93048_1c",
          "table": "Line Items",
          "name": "Invoice",
          "ty": {
            "kind": "integer"
          },
          "required": true,
          "unique": false,
          "default": null,
          "position": 3
        },
        {
          "id": "c_1a20fa93048_1d",
          "table": "Line Items",
          "name": "Amount",
          "ty": {
            "kind": "formula",
            "expression": "ROUND([Quantity] * [Unit Price], 2)"
          },
          "required": false,
          "unique": false,
          "default": null,
          "position": 4
        }
      ],
      "created_at": 1795559075912
    }
  ],
  "relations": [
    {
      "id": "r_1a20fa93048_1e",
      "name": "Client Projects",
      "parent_table": "Clients",
      "child_table": "Projects",
      "child_column": "Client",
      "on_delete": "cascade"
    },
    {
      "id": "r_1a20fa93049_1f",
      "name": "Project Invoices",
      "parent_table": "Projects",
      "child_table": "Invoices",
      "child_column": "Project",
      "on_delete": "cascade"
    },
    {
      "id": "r_1a20fa9304b_20",
      "name": "Client Invoices",
      "parent_table": "Clients",
      "child_table": "Invoices",
      "child_column": "Client",
      "on_delete": "restrict"
    },
    {
      "id": "r_1a20fa9304c_21",
      "name": "Invoice Lines",
      "parent_table": "Invoices",
      "child_table": "Line Items",
      "child_column": "Invoice",
      "on_delete": "cascade"
    }
  ],
  "layouts": [
    {
      "id": "layout_client",
      "name": "Client Record",
      "table": "Clients",
      "grid": {
        "columns": 12,
        "row_height": 28
      },
      "items": [
        {
          "id": "client_s1",
          "kind": {
            "kind": "section",
            "title": "Contact"
          },
          "x": 0,
          "y": 0,
          "w": 12,
          "h": 1
        },
        {
          "id": "client_name",
          "kind": {
            "kind": "field",
            "column": "Name"
          },
          "x": 0,
          "y": 1,
          "w": 6,
          "h": 2
        },
        {
          "id": "client_email",
          "kind": {
            "kind": "field",
            "column": "Email"
          },
          "x": 6,
          "y": 1,
          "w": 6,
          "h": 2
        },
        {
          "id": "client_phone",
          "kind": {
            "kind": "field",
            "column": "Phone"
          },
          "x": 0,
          "y": 4,
          "w": 4,
          "h": 2
        },
        {
          "id": "client_city",
          "kind": {
            "kind": "field",
            "column": "City"
          },
          "x": 4,
          "y": 4,
          "w": 4,
          "h": 2
        },
        {
          "id": "client_s2",
          "kind": {
            "kind": "section",
            "title": "Notes"
          },
          "x": 0,
          "y": 7,
          "w": 12,
          "h": 1
        },
        {
          "id": "client_notes",
          "kind": {
            "kind": "field",
            "column": "Notes"
          },
          "x": 0,
          "y": 8,
          "w": 12,
          "h": 5
        },
        {
          "id": "client_projects",
          "kind": {
            "kind": "portal",
            "relation": "Client Projects",
            "columns": [
              "Name",
              "Status",
              "Due Date"
            ],
            "rows": 5
          },
          "x": 0,
          "y": 14,
          "w": 12,
          "h": 6
        }
      ],
      "updated_at": 1795559075929
    },
    {
      "id": "layout_project",
      "name": "Project Record",
      "table": "Projects",
      "grid": {
        "columns": 12,
        "row_height": 28
      },
      "items": [
        {
          "id": "project_s1",
          "kind": {
            "kind": "section",
            "title": "Project"
          },
          "x": 0,
          "y": 0,
          "w": 12,
          "h": 1
        },
        {
          "id": "project_name",
          "kind": {
            "kind": "field",
            "column": "Name"
          },
          "x": 0,
          "y": 1,
          "w": 8,
          "h": 2
        },
        {
          "id": "project_status",
          "kind": {
            "kind": "field",
            "column": "Status"
          },
          "x": 8,
          "y": 1,
          "w": 4,
          "h": 2
        },
        {
          "id": "project_budget",
          "kind": {
            "kind": "field",
            "column": "Budget"
          },
          "x": 0,
          "y": 4,
          "w": 4,
          "h": 2
        },
        {
          "id": "project_start",
          "kind": {
            "kind": "field",
            "column": "Start Date"
          },
          "x": 4,
          "y": 4,
          "w": 4,
          "h": 2
        },
        {
          "id": "project_due",
          "kind": {
            "kind": "field",
            "column": "Due Date"
          },
          "x": 8,
          "y": 4,
          "w": 4,
          "h": 2
        },
        {
          "id": "project_s2",
          "kind": {
            "kind": "section",
            "title": "Brief"
          },
          "x": 0,
          "y": 7,
          "w": 12,
          "h": 1
        },
        {
          "id": "project_brief",
          "kind": {
            "kind": "field",
            "column": "Brief"
          },
          "x": 0,
          "y": 8,
          "w": 12,
          "h": 4
        }
      ],
      "updated_at": 1795559075929
    },
    {
      "id": "layout_invoice",
      "name": "Invoice",
      "table": "Invoices",
      "grid": {
        "columns": 12,
        "row_height": 28
      },
      "items": [
        {
          "id": "invoice_s1",
          "kind": {
            "kind": "section",
            "title": "Invoice"
          },
          "x": 0,
          "y": 0,
          "w": 12,
          "h": 1
        },
        {
          "id": "invoice_number",
          "kind": {
            "kind": "field",
            "column": "Number"
          },
          "x": 0,
          "y": 1,
          "w": 4,
          "h": 2
        },
        {
          "id": "invoice_status",
          "kind": {
            "kind": "field",
            "column": "Status"
          },
          "x": 4,
          "y": 1,
          "w": 4,
          "h": 2
        },
        {
          "id": "invoice_issued",
          "kind": {
            "kind": "field",
            "column": "Issued"
          },
          "x": 8,
          "y": 1,
          "w": 4,
          "h": 2
        },
        {
          "id": "invoice_client",
          "kind": {
            "kind": "field",
            "column": "Client"
          },
          "x": 0,
          "y": 4,
          "w": 4,
          "h": 2
        },
        {
          "id": "invoice_project",
          "kind": {
            "kind": "field",
            "column": "Project"
          },
          "x": 4,
          "y": 4,
          "w": 4,
          "h": 2
        },
        {
          "id": "invoice_due",
          "kind": {
            "kind": "field",
            "column": "Due"
          },
          "x": 8,
          "y": 4,
          "w": 4,
          "h": 2
        },
        {
          "id": "invoice_tax_rate",
          "kind": {
            "kind": "field",
            "column": "Tax Rate"
          },
          "x": 0,
          "y": 7,
          "w": 4,
          "h": 2
        },
        {
          "id": "invoice_subtotal",
          "kind": {
            "kind": "field",
            "column": "Subtotal"
          },
          "x": 4,
          "y": 7,
          "w": 4,
          "h": 2
        },
        {
          "id": "invoice_tax",
          "kind": {
            "kind": "field",
            "column": "Tax"
          },
          "x": 8,
          "y": 7,
          "w": 4,
          "h": 2
        },
        {
          "id": "invoice_total",
          "kind": {
            "kind": "field",
            "column": "Total"
          },
          "x": 8,
          "y": 10,
          "w": 4,
          "h": 2
        },
        {
          "id": "invoice_lines",
          "kind": {
            "kind": "portal",
            "relation": "Invoice Lines",
            "columns": [
              "Description",
              "Quantity",
              "Unit Price",
              "Amount"
            ],
            "rows": 6
          },
          "x": 0,
          "y": 13,
          "w": 12,
          "h": 8
        }
      ],
      "updated_at": 1795559075930
    }
  ]
};

export const DESIGN_STUDIO_ROWS: Record<string, SlabRecord[]> = {
  "Clients": [
    {
      "id": 1,
      "values": {
        "Name": {
          "Text": "Northlight Studio"
        },
        "Email": {
          "Text": "studio@northlight.example"
        },
        "Phone": {
          "Text": "+1 415 555 0144"
        },
        "City": {
          "Text": "San Francisco"
        },
        "Notes": {
          "Text": "Brand system and packaging work. Net 30."
        }
      }
    },
    {
      "id": 2,
      "values": {
        "Name": {
          "Text": "Hiraya Ceramics"
        },
        "Email": {
          "Text": "hello@hiraya.example"
        },
        "Phone": "Null",
        "City": {
          "Text": "Portland"
        },
        "Notes": {
          "Text": "Seasonal catalogue, photography on site."
        }
      }
    },
    {
      "id": 3,
      "values": {
        "Name": {
          "Text": "Fieldnote Labs"
        },
        "Email": {
          "Text": "ops@fieldnote.example"
        },
        "Phone": "Null",
        "City": {
          "Text": "Austin"
        },
        "Notes": "Null"
      }
    }
  ],
  "Projects": [
    {
      "id": 1,
      "values": {
        "Name": {
          "Text": "Identity refresh"
        },
        "Client": {
          "Integer": 1
        },
        "Status": {
          "Select": "Active"
        },
        "Budget": {
          "Currency": {
            "currency": "USD",
            "amount": "24000.00"
          }
        },
        "Start Date": {
          "Date": "2026-09-01"
        },
        "Due Date": {
          "Date": "2026-12-15"
        },
        "Brief": "Null"
      }
    },
    {
      "id": 2,
      "values": {
        "Name": {
          "Text": "Packaging system"
        },
        "Client": {
          "Integer": 1
        },
        "Status": {
          "Select": "Review"
        },
        "Budget": {
          "Currency": {
            "currency": "USD",
            "amount": "18500.00"
          }
        },
        "Start Date": {
          "Date": "2026-06-10"
        },
        "Due Date": {
          "Date": "2026-11-30"
        },
        "Brief": "Null"
      }
    },
    {
      "id": 3,
      "values": {
        "Name": {
          "Text": "Autumn catalogue"
        },
        "Client": {
          "Integer": 2
        },
        "Status": {
          "Select": "Planning"
        },
        "Budget": {
          "Currency": {
            "currency": "USD",
            "amount": "9200.00"
          }
        },
        "Start Date": {
          "Date": "2026-11-01"
        },
        "Due Date": {
          "Date": "2027-02-20"
        },
        "Brief": "Null"
      }
    },
    {
      "id": 4,
      "values": {
        "Name": {
          "Text": "Field study"
        },
        "Client": {
          "Integer": 3
        },
        "Status": {
          "Select": "Complete"
        },
        "Budget": {
          "Currency": {
            "currency": "USD",
            "amount": "6000.00"
          }
        },
        "Start Date": {
          "Date": "2026-03-01"
        },
        "Due Date": {
          "Date": "2026-05-15"
        },
        "Brief": "Null"
      }
    }
  ],
  "Invoices": [
    {
      "id": 1,
      "values": {
        "Number": {
          "Text": "INV-2026-001"
        },
        "Client": {
          "Integer": 1
        },
        "Project": {
          "Integer": 1
        },
        "Issued": {
          "Date": "2026-09-15"
        },
        "Due": {
          "Date": "2026-10-15"
        },
        "Status": {
          "Select": "Paid"
        },
        "Tax Rate": {
          "Decimal": "0.0825"
        },
        "Note": "Null",
        "Subtotal": {
          "Currency": {
            "currency": "USD",
            "amount": "20200.00"
          }
        },
        "Tax": {
          "Currency": {
            "currency": "USD",
            "amount": "1666.50"
          }
        },
        "Total": {
          "Currency": {
            "currency": "USD",
            "amount": "21866.50"
          }
        }
      }
    },
    {
      "id": 2,
      "values": {
        "Number": {
          "Text": "INV-2026-002"
        },
        "Client": {
          "Integer": 1
        },
        "Project": {
          "Integer": 2
        },
        "Issued": {
          "Date": "2026-10-02"
        },
        "Due": {
          "Date": "2026-11-01"
        },
        "Status": {
          "Select": "Sent"
        },
        "Tax Rate": {
          "Decimal": "0.0825"
        },
        "Note": "Null",
        "Subtotal": {
          "Currency": {
            "currency": "USD",
            "amount": "9480.00"
          }
        },
        "Tax": {
          "Currency": {
            "currency": "USD",
            "amount": "782.10"
          }
        },
        "Total": {
          "Currency": {
            "currency": "USD",
            "amount": "10262.10"
          }
        }
      }
    },
    {
      "id": 3,
      "values": {
        "Number": {
          "Text": "INV-2026-003"
        },
        "Client": {
          "Integer": 2
        },
        "Project": {
          "Integer": 3
        },
        "Issued": {
          "Date": "2026-11-12"
        },
        "Due": {
          "Date": "2026-12-12"
        },
        "Status": {
          "Select": "Draft"
        },
        "Tax Rate": {
          "Decimal": "0.0000"
        },
        "Note": "Null",
        "Subtotal": {
          "Currency": {
            "currency": "USD",
            "amount": "4890.00"
          }
        },
        "Tax": {
          "Currency": {
            "currency": "USD",
            "amount": "0.00"
          }
        },
        "Total": {
          "Currency": {
            "currency": "USD",
            "amount": "4890.00"
          }
        }
      }
    },
    {
      "id": 4,
      "values": {
        "Number": {
          "Text": "INV-2026-004"
        },
        "Client": {
          "Integer": 3
        },
        "Project": {
          "Integer": 4
        },
        "Issued": {
          "Date": "2026-05-20"
        },
        "Due": {
          "Date": "2026-06-19"
        },
        "Status": {
          "Select": "Paid"
        },
        "Tax Rate": {
          "Decimal": "0.0825"
        },
        "Note": "Null",
        "Subtotal": {
          "Currency": {
            "currency": "USD",
            "amount": "4960.00"
          }
        },
        "Tax": {
          "Currency": {
            "currency": "USD",
            "amount": "409.20"
          }
        },
        "Total": {
          "Currency": {
            "currency": "USD",
            "amount": "5369.20"
          }
        }
      }
    }
  ],
  "Line Items": [
    {
      "id": 1,
      "values": {
        "Description": {
          "Text": "Discovery workshop"
        },
        "Quantity": {
          "Decimal": "2.00"
        },
        "Unit Price": {
          "Currency": {
            "currency": "USD",
            "amount": "1400.00"
          }
        },
        "Invoice": {
          "Integer": 1
        },
        "Amount": {
          "Currency": {
            "currency": "USD",
            "amount": "2800.00"
          }
        }
      }
    },
    {
      "id": 2,
      "values": {
        "Description": {
          "Text": "Identity directions"
        },
        "Quantity": {
          "Decimal": "3.00"
        },
        "Unit Price": {
          "Currency": {
            "currency": "USD",
            "amount": "3200.00"
          }
        },
        "Invoice": {
          "Integer": 1
        },
        "Amount": {
          "Currency": {
            "currency": "USD",
            "amount": "9600.00"
          }
        }
      }
    },
    {
      "id": 3,
      "values": {
        "Description": {
          "Text": "Design system build"
        },
        "Quantity": {
          "Decimal": "1.00"
        },
        "Unit Price": {
          "Currency": {
            "currency": "USD",
            "amount": "7800.00"
          }
        },
        "Invoice": {
          "Integer": 1
        },
        "Amount": {
          "Currency": {
            "currency": "USD",
            "amount": "7800.00"
          }
        }
      }
    },
    {
      "id": 4,
      "values": {
        "Description": {
          "Text": "Structural design"
        },
        "Quantity": {
          "Decimal": "4.00"
        },
        "Unit Price": {
          "Currency": {
            "currency": "USD",
            "amount": "1450.00"
          }
        },
        "Invoice": {
          "Integer": 2
        },
        "Amount": {
          "Currency": {
            "currency": "USD",
            "amount": "5800.00"
          }
        }
      }
    },
    {
      "id": 5,
      "values": {
        "Description": {
          "Text": "Print production files"
        },
        "Quantity": {
          "Decimal": "1.00"
        },
        "Unit Price": {
          "Currency": {
            "currency": "USD",
            "amount": "2600.00"
          }
        },
        "Invoice": {
          "Integer": 2
        },
        "Amount": {
          "Currency": {
            "currency": "USD",
            "amount": "2600.00"
          }
        }
      }
    },
    {
      "id": 6,
      "values": {
        "Description": {
          "Text": "Proofing"
        },
        "Quantity": {
          "Decimal": "6.00"
        },
        "Unit Price": {
          "Currency": {
            "currency": "USD",
            "amount": "180.00"
          }
        },
        "Invoice": {
          "Integer": 2
        },
        "Amount": {
          "Currency": {
            "currency": "USD",
            "amount": "1080.00"
          }
        }
      }
    },
    {
      "id": 7,
      "values": {
        "Description": {
          "Text": "Art direction"
        },
        "Quantity": {
          "Decimal": "3.00"
        },
        "Unit Price": {
          "Currency": {
            "currency": "USD",
            "amount": "1250.00"
          }
        },
        "Invoice": {
          "Integer": 3
        },
        "Amount": {
          "Currency": {
            "currency": "USD",
            "amount": "3750.00"
          }
        }
      }
    },
    {
      "id": 8,
      "values": {
        "Description": {
          "Text": "Retouching"
        },
        "Quantity": {
          "Decimal": "12.00"
        },
        "Unit Price": {
          "Currency": {
            "currency": "USD",
            "amount": "95.00"
          }
        },
        "Invoice": {
          "Integer": 3
        },
        "Amount": {
          "Currency": {
            "currency": "USD",
            "amount": "1140.00"
          }
        }
      }
    },
    {
      "id": 9,
      "values": {
        "Description": {
          "Text": "Field interviews"
        },
        "Quantity": {
          "Decimal": "8.00"
        },
        "Unit Price": {
          "Currency": {
            "currency": "USD",
            "amount": "320.00"
          }
        },
        "Invoice": {
          "Integer": 4
        },
        "Amount": {
          "Currency": {
            "currency": "USD",
            "amount": "2560.00"
          }
        }
      }
    },
    {
      "id": 10,
      "values": {
        "Description": {
          "Text": "Synthesis report"
        },
        "Quantity": {
          "Decimal": "1.00"
        },
        "Unit Price": {
          "Currency": {
            "currency": "USD",
            "amount": "2400.00"
          }
        },
        "Invoice": {
          "Integer": 4
        },
        "Amount": {
          "Currency": {
            "currency": "USD",
            "amount": "2400.00"
          }
        }
      }
    }
  ]
};

export const DESIGN_STUDIO_AUDIT: AuditEntry[] = [
  {
    "id": 1,
    "hlc": "0001795559075910-000000-fixture",
    "timestamp_ms": 1795559075910,
    "actor": "local",
    "transaction": "tx-0001795559075910-000000-fixture",
    "table": "Clients",
    "row_id": null,
    "operation": "create_table",
    "summary": "Create table Clients",
    "schema_change": true
  },
  {
    "id": 2,
    "hlc": "0001795559075911-000000-fixture",
    "timestamp_ms": 1795559075911,
    "actor": "local",
    "transaction": "tx-0001795559075911-000000-fixture",
    "table": "Projects",
    "row_id": null,
    "operation": "create_table",
    "summary": "Create table Projects",
    "schema_change": true
  },
  {
    "id": 3,
    "hlc": "0001795559075911-000001-fixture",
    "timestamp_ms": 1795559075911,
    "actor": "local",
    "transaction": "tx-0001795559075911-000001-fixture",
    "table": "Invoices",
    "row_id": null,
    "operation": "create_table",
    "summary": "Create table Invoices",
    "schema_change": true
  },
  {
    "id": 4,
    "hlc": "0001795559075912-000000-fixture",
    "timestamp_ms": 1795559075912,
    "actor": "local",
    "transaction": "tx-0001795559075912-000000-fixture",
    "table": "Line Items",
    "row_id": null,
    "operation": "create_table",
    "summary": "Create table Line Items",
    "schema_change": true
  },
  {
    "id": 5,
    "hlc": "0001795559075913-000000-fixture",
    "timestamp_ms": 1795559075913,
    "actor": "local",
    "transaction": "tx-0001795559075913-000000-fixture",
    "table": "Projects",
    "row_id": null,
    "operation": "create_relation",
    "summary": "Create relation Client Projects",
    "schema_change": true
  },
  {
    "id": 6,
    "hlc": "0001795559075914-000000-fixture",
    "timestamp_ms": 1795559075914,
    "actor": "local",
    "transaction": "tx-0001795559075914-000000-fixture",
    "table": "Invoices",
    "row_id": null,
    "operation": "create_relation",
    "summary": "Create relation Project Invoices",
    "schema_change": true
  },
  {
    "id": 7,
    "hlc": "0001795559075915-000000-fixture",
    "timestamp_ms": 1795559075915,
    "actor": "local",
    "transaction": "tx-0001795559075915-000000-fixture",
    "table": "Invoices",
    "row_id": null,
    "operation": "create_relation",
    "summary": "Create relation Client Invoices",
    "schema_change": true
  },
  {
    "id": 8,
    "hlc": "0001795559075916-000000-fixture",
    "timestamp_ms": 1795559075916,
    "actor": "local",
    "transaction": "tx-0001795559075916-000000-fixture",
    "table": "Line Items",
    "row_id": null,
    "operation": "create_relation",
    "summary": "Create relation Invoice Lines",
    "schema_change": true
  },
  {
    "id": 9,
    "hlc": "0001795559075917-000000-fixture",
    "timestamp_ms": 1795559075917,
    "actor": "local",
    "transaction": "tx-0001795559075917-000000-fixture",
    "table": "Invoices",
    "row_id": null,
    "operation": "add_column",
    "summary": "Add Invoices.Subtotal",
    "schema_change": true
  },
  {
    "id": 10,
    "hlc": "0001795559075917-000001-fixture",
    "timestamp_ms": 1795559075917,
    "actor": "local",
    "transaction": "tx-0001795559075917-000001-fixture",
    "table": "Invoices",
    "row_id": null,
    "operation": "add_column",
    "summary": "Add Invoices.Tax",
    "schema_change": true
  },
  {
    "id": 11,
    "hlc": "0001795559075917-000002-fixture",
    "timestamp_ms": 1795559075917,
    "actor": "local",
    "transaction": "tx-0001795559075917-000002-fixture",
    "table": "Invoices",
    "row_id": null,
    "operation": "add_column",
    "summary": "Add Invoices.Total",
    "schema_change": true
  },
  {
    "id": 12,
    "hlc": "0001795559075918-000000-fixture",
    "timestamp_ms": 1795559075918,
    "actor": "local",
    "transaction": "tx-0001795559075918-000000-fixture",
    "table": "Clients",
    "row_id": 1,
    "operation": "insert",
    "summary": "Insert Clients #0",
    "schema_change": false
  },
  {
    "id": 13,
    "hlc": "0001795559075918-000001-fixture",
    "timestamp_ms": 1795559075918,
    "actor": "local",
    "transaction": "tx-0001795559075918-000001-fixture",
    "table": "Clients",
    "row_id": 2,
    "operation": "insert",
    "summary": "Insert Clients #0",
    "schema_change": false
  },
  {
    "id": 14,
    "hlc": "0001795559075919-000000-fixture",
    "timestamp_ms": 1795559075919,
    "actor": "local",
    "transaction": "tx-0001795559075919-000000-fixture",
    "table": "Clients",
    "row_id": 3,
    "operation": "insert",
    "summary": "Insert Clients #0",
    "schema_change": false
  },
  {
    "id": 15,
    "hlc": "0001795559075919-000001-fixture",
    "timestamp_ms": 1795559075919,
    "actor": "local",
    "transaction": "tx-0001795559075919-000001-fixture",
    "table": "Projects",
    "row_id": 1,
    "operation": "insert",
    "summary": "Insert Projects #0",
    "schema_change": false
  },
  {
    "id": 16,
    "hlc": "0001795559075920-000000-fixture",
    "timestamp_ms": 1795559075920,
    "actor": "local",
    "transaction": "tx-0001795559075920-000000-fixture",
    "table": "Projects",
    "row_id": 2,
    "operation": "insert",
    "summary": "Insert Projects #0",
    "schema_change": false
  },
  {
    "id": 17,
    "hlc": "0001795559075920-000001-fixture",
    "timestamp_ms": 1795559075920,
    "actor": "local",
    "transaction": "tx-0001795559075920-000001-fixture",
    "table": "Projects",
    "row_id": 3,
    "operation": "insert",
    "summary": "Insert Projects #0",
    "schema_change": false
  },
  {
    "id": 18,
    "hlc": "0001795559075921-000000-fixture",
    "timestamp_ms": 1795559075921,
    "actor": "local",
    "transaction": "tx-0001795559075921-000000-fixture",
    "table": "Projects",
    "row_id": 4,
    "operation": "insert",
    "summary": "Insert Projects #0",
    "schema_change": false
  },
  {
    "id": 19,
    "hlc": "0001795559075921-000001-fixture",
    "timestamp_ms": 1795559075921,
    "actor": "local",
    "transaction": "tx-0001795559075921-000001-fixture",
    "table": "Invoices",
    "row_id": 1,
    "operation": "insert",
    "summary": "Insert Invoices #0",
    "schema_change": false
  },
  {
    "id": 20,
    "hlc": "0001795559075922-000000-fixture",
    "timestamp_ms": 1795559075922,
    "actor": "local",
    "transaction": "tx-0001795559075922-000000-fixture",
    "table": "Invoices",
    "row_id": 2,
    "operation": "insert",
    "summary": "Insert Invoices #0",
    "schema_change": false
  },
  {
    "id": 21,
    "hlc": "0001795559075923-000000-fixture",
    "timestamp_ms": 1795559075923,
    "actor": "local",
    "transaction": "tx-0001795559075923-000000-fixture",
    "table": "Invoices",
    "row_id": 3,
    "operation": "insert",
    "summary": "Insert Invoices #0",
    "schema_change": false
  },
  {
    "id": 22,
    "hlc": "0001795559075923-000001-fixture",
    "timestamp_ms": 1795559075923,
    "actor": "local",
    "transaction": "tx-0001795559075923-000001-fixture",
    "table": "Invoices",
    "row_id": 4,
    "operation": "insert",
    "summary": "Insert Invoices #0",
    "schema_change": false
  },
  {
    "id": 23,
    "hlc": "0001795559075924-000000-fixture",
    "timestamp_ms": 1795559075924,
    "actor": "local",
    "transaction": "tx-0001795559075924-000000-fixture",
    "table": "Line Items",
    "row_id": 1,
    "operation": "insert",
    "summary": "Insert Line Items #0",
    "schema_change": false
  },
  {
    "id": 24,
    "hlc": "0001795559075924-000001-fixture",
    "timestamp_ms": 1795559075924,
    "actor": "local",
    "transaction": "tx-0001795559075924-000001-fixture",
    "table": "Line Items",
    "row_id": 2,
    "operation": "insert",
    "summary": "Insert Line Items #0",
    "schema_change": false
  },
  {
    "id": 25,
    "hlc": "0001795559075925-000000-fixture",
    "timestamp_ms": 1795559075925,
    "actor": "local",
    "transaction": "tx-0001795559075925-000000-fixture",
    "table": "Line Items",
    "row_id": 3,
    "operation": "insert",
    "summary": "Insert Line Items #0",
    "schema_change": false
  },
  {
    "id": 26,
    "hlc": "0001795559075925-000001-fixture",
    "timestamp_ms": 1795559075925,
    "actor": "local",
    "transaction": "tx-0001795559075925-000001-fixture",
    "table": "Line Items",
    "row_id": 4,
    "operation": "insert",
    "summary": "Insert Line Items #0",
    "schema_change": false
  },
  {
    "id": 27,
    "hlc": "0001795559075926-000000-fixture",
    "timestamp_ms": 1795559075926,
    "actor": "local",
    "transaction": "tx-0001795559075926-000000-fixture",
    "table": "Line Items",
    "row_id": 5,
    "operation": "insert",
    "summary": "Insert Line Items #0",
    "schema_change": false
  },
  {
    "id": 28,
    "hlc": "0001795559075926-000001-fixture",
    "timestamp_ms": 1795559075926,
    "actor": "local",
    "transaction": "tx-0001795559075926-000001-fixture",
    "table": "Line Items",
    "row_id": 6,
    "operation": "insert",
    "summary": "Insert Line Items #0",
    "schema_change": false
  },
  {
    "id": 29,
    "hlc": "0001795559075927-000000-fixture",
    "timestamp_ms": 1795559075927,
    "actor": "local",
    "transaction": "tx-0001795559075927-000000-fixture",
    "table": "Line Items",
    "row_id": 7,
    "operation": "insert",
    "summary": "Insert Line Items #0",
    "schema_change": false
  },
  {
    "id": 30,
    "hlc": "0001795559075927-000001-fixture",
    "timestamp_ms": 1795559075927,
    "actor": "local",
    "transaction": "tx-0001795559075927-000001-fixture",
    "table": "Line Items",
    "row_id": 8,
    "operation": "insert",
    "summary": "Insert Line Items #0",
    "schema_change": false
  },
  {
    "id": 31,
    "hlc": "0001795559075928-000000-fixture",
    "timestamp_ms": 1795559075928,
    "actor": "local",
    "transaction": "tx-0001795559075928-000000-fixture",
    "table": "Line Items",
    "row_id": 9,
    "operation": "insert",
    "summary": "Insert Line Items #0",
    "schema_change": false
  },
  {
    "id": 32,
    "hlc": "0001795559075928-000001-fixture",
    "timestamp_ms": 1795559075928,
    "actor": "local",
    "transaction": "tx-0001795559075928-000001-fixture",
    "table": "Line Items",
    "row_id": 10,
    "operation": "insert",
    "summary": "Insert Line Items #0",
    "schema_change": false
  },
  {
    "id": 33,
    "hlc": "0001795559075929-000000-fixture",
    "timestamp_ms": 1795559075929,
    "actor": "local",
    "transaction": "tx-0001795559075929-000000-fixture",
    "table": "Clients",
    "row_id": null,
    "operation": "save_layout",
    "summary": "Save form Client Record",
    "schema_change": true
  },
  {
    "id": 34,
    "hlc": "0001795559075929-000001-fixture",
    "timestamp_ms": 1795559075929,
    "actor": "local",
    "transaction": "tx-0001795559075929-000001-fixture",
    "table": "Projects",
    "row_id": null,
    "operation": "save_layout",
    "summary": "Save form Project Record",
    "schema_change": true
  },
  {
    "id": 35,
    "hlc": "0001795559075930-000000-fixture",
    "timestamp_ms": 1795559075930,
    "actor": "local",
    "transaction": "tx-0001795559075930-000000-fixture",
    "table": "Invoices",
    "row_id": null,
    "operation": "save_layout",
    "summary": "Save form Invoice",
    "schema_change": true
  }
];
