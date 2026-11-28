use rusqlite::{Connection, Transaction, params};
use slab_core::{
    Column, ColumnDraft, ColumnType, Command, DeleteAction, IndexSpec, Layout, LayoutItemKind,
    Relation, RelationDraft, RowId, RowValue, Schema, Table, TableDraft, TableSummary,
    validate_identifier,
};
use slab_formula::{FormulaDag, parse as parse_formula, rewrite_column_source};

use crate::ddl::{column_definition, create_table_sql, relation_index_sql};
use crate::error::StorageError;
use crate::sql::{decode_value, quote_ident, stored_columns};

use super::records::read_stored_record;
use super::{Workspace, now_ms};

impl Workspace {
    /// Tables and row counts for the sidebar.
    pub fn table_summaries(&self) -> Result<Vec<TableSummary>, StorageError> {
        let schema = self.schema()?;
        let mut summaries = Vec::with_capacity(schema.tables.len());
        for table in &schema.tables {
            let sql = format!("SELECT COUNT(*) FROM {}", quote_ident(&table.name));
            let count: i64 = self.conn.query_row(&sql, [], |row| row.get(0))?;
            summaries.push(TableSummary {
                name: table.name.clone(),
                row_count: u64::try_from(count).unwrap_or(0),
            });
        }
        Ok(summaries)
    }

    /// Creates a table from a draft, rejecting invalid formulas before any DDL.
    pub fn create_table(&mut self, draft: &TableDraft) -> Result<Table, StorageError> {
        validate_identifier(&draft.name)?;
        let mut table = Table {
            id: self.next_id("t"),
            name: draft.name.clone(),
            columns: Vec::with_capacity(draft.columns.len()),
            created_at: now_ms(),
        };
        for (index, column) in draft.columns.iter().enumerate() {
            table.columns.push(Column {
                id: self.next_id("c"),
                table: draft.name.clone(),
                name: column.name.clone(),
                ty: column.ty.clone(),
                required: column.required,
                unique: column.unique,
                default: column.default.clone(),
                position: u32::try_from(index).unwrap_or(0),
            });
        }
        table.validate()?;
        let mut candidate = self.schema()?;
        if candidate.table(&table.name).is_some() {
            return Err(StorageError::Unique(format!(
                "table {} already exists",
                table.name
            )));
        }
        candidate.tables.push(table.clone());
        candidate.validate()?;
        FormulaDag::build(&candidate)?;
        let command = Command::CreateTable {
            table: table.clone(),
        };
        self.apply_command(&command)?;
        Ok(table)
    }

    /// Adds a column, rebuilding no tables.
    pub fn add_column(
        &mut self,
        table_name: &str,
        draft: &ColumnDraft,
    ) -> Result<Column, StorageError> {
        let schema = self.schema()?;
        let table = schema
            .table(table_name)
            .ok_or_else(|| StorageError::NotFound(table_name.to_owned()))?;
        let position = u32::try_from(table.columns.len()).unwrap_or(0);
        let column = Column {
            id: self.next_id("c"),
            table: table_name.to_owned(),
            name: draft.name.clone(),
            ty: draft.ty.clone(),
            required: draft.required,
            unique: draft.unique,
            default: draft.default.clone(),
            position,
        };
        column.validate()?;
        let mut candidate = schema.clone();
        let table = candidate
            .table_mut(table_name)
            .ok_or_else(|| StorageError::NotFound(table_name.to_owned()))?;
        if table.column(&column.name).is_some() {
            return Err(StorageError::Unique(format!(
                "column {}.{} already exists",
                table_name, column.name
            )));
        }
        table.columns.push(column.clone());
        candidate.validate()?;
        FormulaDag::build(&candidate)?;
        let command = Command::AddColumn {
            table: table_name.to_owned(),
            column: column.clone(),
        };
        self.apply_command(&command)?;
        Ok(column)
    }

    /// Creates a parent-to-child relation, rebuilding the child table with a
    /// real foreign key.
    pub fn create_relation(&mut self, draft: &RelationDraft) -> Result<Relation, StorageError> {
        if draft.on_delete == DeleteAction::SetNull {
            return Err(StorageError::Unsupported(
                "SET NULL relations are reserved for a later release".into(),
            ));
        }
        let relation = Relation {
            id: self.next_id("r"),
            name: draft.name.clone(),
            parent_table: draft.parent_table.clone(),
            child_table: draft.child_table.clone(),
            child_column: draft.child_column.clone(),
            on_delete: draft.on_delete,
        };
        relation.validate()?;
        let mut candidate = self.schema()?;
        if candidate.relation(&relation.name).is_some() {
            return Err(StorageError::Unique(format!(
                "relation {} already exists",
                relation.name
            )));
        }
        let child = candidate
            .table(&relation.child_table)
            .ok_or_else(|| StorageError::NotFound(relation.child_table.clone()))?;
        let column = child.column(&relation.child_column).ok_or_else(|| {
            StorageError::Query(format!(
                "missing child column {}.{}",
                relation.child_table, relation.child_column
            ))
        })?;
        if !matches!(column.ty, ColumnType::Integer) {
            return Err(StorageError::Command(
                "relation columns must be Integer".into(),
            ));
        }
        if !column.ty.is_stored() {
            return Err(StorageError::Command(
                "relation columns must be stored".into(),
            ));
        }
        if candidate.relations.iter().any(|existing| {
            existing.child_table == relation.child_table
                && existing.child_column == relation.child_column
        }) {
            return Err(StorageError::Command(format!(
                "{} already has a relation on {}",
                relation.child_table, relation.child_column
            )));
        }
        candidate.relations.push(relation.clone());
        candidate.validate()?;
        let command = Command::CreateRelation {
            relation: relation.clone(),
        };
        self.apply_command(&command)?;
        Ok(relation)
    }

    /// Removes a relation and its foreign key constraint.
    pub fn drop_relation(&mut self, name: &str) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let relation = schema
            .relation(name)
            .ok_or_else(|| StorageError::NotFound(name.to_owned()))?
            .clone();
        let command = Command::DropRelation { relation };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Renames a table and every metadata reference to it.
    pub fn rename_table(&mut self, old_name: &str, new_name: &str) -> Result<(), StorageError> {
        validate_identifier(new_name)?;
        let schema = self.schema()?;
        if schema.table(old_name).is_none() {
            return Err(StorageError::NotFound(old_name.to_owned()));
        }
        if schema.table(new_name).is_some() {
            return Err(StorageError::Unique(format!(
                "table {new_name} already exists"
            )));
        }
        let mut candidate = schema.clone();
        let table = candidate
            .table_mut(old_name)
            .ok_or_else(|| StorageError::NotFound(old_name.to_owned()))?;
        table.name = new_name.to_owned();
        for column in &mut table.columns {
            column.table = new_name.to_owned();
        }
        for relation in &mut candidate.relations {
            if relation.parent_table == old_name {
                relation.parent_table = new_name.to_owned();
            }
            if relation.child_table == old_name {
                relation.child_table = new_name.to_owned();
            }
        }
        for layout in &mut candidate.layouts {
            if layout.table == old_name {
                layout.table = new_name.to_owned();
            }
        }
        candidate.validate()?;
        FormulaDag::build(&candidate)?;
        let command = Command::RenameTable {
            old_name: old_name.to_owned(),
            new_name: new_name.to_owned(),
        };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Renames a column and rewrites formulas, lookups, layouts, and relations.
    pub fn rename_column(
        &mut self,
        table: &str,
        old_name: &str,
        new_name: &str,
    ) -> Result<(), StorageError> {
        validate_identifier(new_name)?;
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        if definition.column(old_name).is_none() {
            return Err(StorageError::NotFound(format!("{table}.{old_name}")));
        }
        if definition.column(new_name).is_some() {
            return Err(StorageError::Unique(format!(
                "{table}.{new_name} already exists"
            )));
        }
        rename_column_in_schema(&schema, table, old_name, new_name)?;
        let command = Command::RenameColumn {
            table: table.to_owned(),
            old_name: old_name.to_owned(),
            new_name: new_name.to_owned(),
        };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Drops a table and retains its rows and layouts for undo.
    pub fn drop_table(&mut self, name: &str) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let table = schema
            .table(name)
            .ok_or_else(|| StorageError::NotFound(name.to_owned()))?
            .clone();
        if let Some(relation) = schema
            .relations
            .iter()
            .find(|relation| relation.parent_table == name || relation.child_table == name)
        {
            return Err(StorageError::Command(format!(
                "table {name} is used by relation {}; drop the relation first",
                relation.name
            )));
        }
        let rows = stored_records(&self.conn, &table)?;
        let layouts = schema
            .layouts
            .iter()
            .filter(|layout| layout.table == name)
            .cloned()
            .collect();
        let indexes = list_user_indexes(&self.conn, name)?;
        let command = Command::DropTable {
            table,
            rows,
            layouts,
            indexes,
        };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Drops a column after blocking formula, relation, and layout references.
    pub fn drop_column(&mut self, table: &str, column: &str) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let column_definition = definition
            .column(column)
            .ok_or_else(|| StorageError::NotFound(format!("{table}.{column}")))?
            .clone();
        if let Some(relation) = schema
            .relations
            .iter()
            .find(|relation| relation.child_table == table && relation.child_column == column)
        {
            return Err(StorageError::Reference(format!(
                "column {table}.{column} is used by relation {}; drop the relation first",
                relation.name
            )));
        }
        if let Some(reference) = formula_reference(&schema, table, column)? {
            return Err(StorageError::Reference(format!(
                "column {table}.{column} is referenced by {reference}"
            )));
        }
        if let Some(layout) = layout_reference(&schema, table, column) {
            return Err(StorageError::Reference(format!(
                "column {table}.{column} is used by form {layout}"
            )));
        }
        let values = stored_values(&self.conn, definition, &column_definition)?;
        let command = Command::DropColumn {
            table: table.to_owned(),
            column: column_definition,
            values,
        };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Lists user-created single-column indexes for a table.
    pub fn list_indexes(&self, table: &str) -> Result<Vec<IndexSpec>, StorageError> {
        let schema = self.schema()?;
        if schema.table(table).is_none() {
            return Err(StorageError::NotFound(table.to_owned()));
        }
        list_user_indexes(&self.conn, table)
    }

    /// Creates a single-column index if it does not already exist.
    pub fn create_index(&mut self, table: &str, column: &str) -> Result<IndexSpec, StorageError> {
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let column_definition = definition
            .column(column)
            .ok_or_else(|| StorageError::NotFound(format!("{table}.{column}")))?;
        if !column_definition.ty.is_stored() {
            return Err(StorageError::Command(
                "computed columns cannot be indexed".into(),
            ));
        }
        let existing = list_user_indexes(&self.conn, table)?;
        if let Some(index) = existing
            .into_iter()
            .find(|index| index.columns == vec![column.to_owned()])
        {
            return Ok(index);
        }
        let index = IndexSpec {
            name: format!("slab_idx_{table}_{column}").replace(' ', "_"),
            table: table.to_owned(),
            columns: vec![column.to_owned()],
            unique: false,
        };
        let command = Command::CreateIndex {
            index: index.clone(),
        };
        self.apply_command(&command)?;
        Ok(index)
    }

    /// Drops a user-created index.
    pub fn drop_index(&mut self, name: &str) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let mut found = None;
        for table in &schema.tables {
            if let Some(index) = list_user_indexes(&self.conn, &table.name)?
                .into_iter()
                .find(|index| index.name == name)
            {
                found = Some(index);
                break;
            }
        }
        let index = found.ok_or_else(|| StorageError::NotFound(format!("index {name}")))?;
        let command = Command::DropIndex { index };
        self.apply_command(&command)?;
        Ok(())
    }

    /// Inserts or updates a form layout.
    pub fn save_layout(&mut self, mut layout: Layout) -> Result<Layout, StorageError> {
        let schema = self.schema()?;
        if layout.id.is_empty() {
            layout.id = self.next_id("l");
        }
        layout.updated_at = now_ms();
        layout.validate(&schema)?;
        let before = schema
            .layouts
            .iter()
            .find(|existing| existing.id == layout.id)
            .cloned();
        let command = Command::SaveLayout {
            before,
            after: layout.clone(),
        };
        self.apply_command(&command)?;
        Ok(layout)
    }

    /// Deletes a form layout.
    pub fn delete_layout(&mut self, id: &str) -> Result<(), StorageError> {
        let schema = self.schema()?;
        let layout = schema
            .layouts
            .iter()
            .find(|layout| layout.id == id)
            .cloned()
            .ok_or_else(|| StorageError::NotFound(format!("layout {id}")))?;
        let command = Command::DeleteLayout { layout };
        self.apply_command(&command)?;
        Ok(())
    }
}

pub(super) fn insert_table_meta(tx: &Transaction<'_>, table: &Table) -> Result<(), StorageError> {
    tx.execute(
        "INSERT INTO _slab_tables (id, name, created_at) VALUES (?1, ?2, ?3)",
        params![table.id, table.name, table.created_at],
    )?;
    for column in &table.columns {
        insert_column_meta(tx, column)?;
    }
    Ok(())
}

pub(super) fn insert_column_meta(
    tx: &Transaction<'_>,
    column: &Column,
) -> Result<(), StorageError> {
    tx.execute(
        "INSERT INTO _slab_columns
            (id, table_name, name, type_json, required, unique_flag, default_json, position)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            column.id,
            column.table,
            column.name,
            serde_json::to_string(&column.ty)?,
            i64::from(column.required),
            i64::from(column.unique),
            column
                .default
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            i64::from(column.position),
        ],
    )?;
    Ok(())
}

pub(super) fn insert_layout_meta(
    tx: &Transaction<'_>,
    layout: &Layout,
) -> Result<(), StorageError> {
    tx.execute(
        "INSERT INTO _slab_layouts (id, name, table_name, spec_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            layout.id,
            layout.name,
            layout.table,
            serde_json::to_string(layout)?,
            layout.updated_at
        ],
    )?;
    Ok(())
}

pub(super) fn create_index_sql(index: &IndexSpec) -> String {
    format!(
        "CREATE {}INDEX IF NOT EXISTS {} ON {} ({})",
        if index.unique { "UNIQUE " } else { "" },
        quote_ident(&index.name),
        quote_ident(&index.table),
        index
            .columns
            .iter()
            .map(|column| quote_ident(column))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

pub(super) fn insert_relation_meta(
    tx: &Transaction<'_>,
    relation: &Relation,
) -> Result<(), StorageError> {
    tx.execute(
        "INSERT INTO _slab_relations (id, name, parent_table, child_table, child_column, on_delete)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            relation.id,
            relation.name,
            relation.parent_table,
            relation.child_table,
            relation.child_column,
            match relation.on_delete {
                DeleteAction::Cascade => "cascade",
                DeleteAction::SetNull => "setNull",
                DeleteAction::Restrict => "restrict",
            },
        ],
    )?;
    Ok(())
}

pub(super) fn column_definition_alter(column: &Column) -> Result<String, StorageError> {
    let definition = column_definition(column)?;
    // ALTER TABLE cannot add a UNIQUE column; a unique index is created after.
    Ok(definition.replace(" UNIQUE", ""))
}

pub(super) fn ensure_relation_values_valid(
    tx: &Transaction<'_>,
    child_table: &str,
    child_column: &str,
    parent_table: &str,
) -> Result<(), StorageError> {
    let sql = format!(
        "SELECT COUNT(*) FROM {} AS child
         LEFT JOIN {} AS parent ON child.{} = parent.{}
         WHERE child.{} IS NOT NULL AND parent.{} IS NULL",
        quote_ident(child_table),
        quote_ident(parent_table),
        quote_ident(child_column),
        quote_ident("id"),
        quote_ident(child_column),
        quote_ident("id")
    );
    let violations: i64 = tx.query_row(&sql, [], |row| row.get(0))?;
    if violations > 0 {
        return Err(StorageError::ForeignKey(format!(
            "{violations} child rows in {child_table}.{child_column} have no matching parent"
        )));
    }
    Ok(())
}

pub(super) fn rebuild_table(
    tx: &Transaction<'_>,
    table: &Table,
    relations: &[Relation],
) -> Result<(), StorageError> {
    let existing_indexes = list_user_indexes(tx, &table.name)?;
    let temporary = format!("{}__slab_rebuild", table.name);
    tx.execute_batch(&format!("DROP TABLE IF EXISTS {}", quote_ident(&temporary)))?;
    let create = create_table_sql(table, &temporary, relations)?;
    tx.execute_batch(&create)?;
    let columns = stored_columns(table);
    let column_list = {
        let mut names = vec![quote_ident("id")];
        names.extend(columns.iter().map(|column| quote_ident(&column.name)));
        names.join(", ")
    };
    tx.execute_batch(&format!(
        "INSERT INTO {} ({}) SELECT {} FROM {}",
        quote_ident(&temporary),
        column_list,
        column_list,
        quote_ident(&table.name)
    ))?;
    tx.execute_batch(&format!("DROP TABLE {}", quote_ident(&table.name)))?;
    tx.execute_batch(&format!(
        "ALTER TABLE {} RENAME TO {}",
        quote_ident(&temporary),
        quote_ident(&table.name)
    ))?;
    for relation in relations
        .iter()
        .filter(|relation| relation.child_table == table.name)
    {
        tx.execute_batch(&relation_index_sql(&table.name, &relation.child_column))?;
    }
    for index in existing_indexes {
        if index
            .columns
            .iter()
            .all(|name| table.column(name).is_some())
        {
            tx.execute_batch(&create_index_sql(&index))?;
        }
    }
    let violations: i64 =
        tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
            row.get(0)
        })?;
    if violations > 0 {
        return Err(StorageError::ForeignKey(format!(
            "{violations} rows violate foreign keys after rebuilding {}",
            table.name
        )));
    }
    Ok(())
}

pub(super) fn rename_column_in_schema(
    schema: &Schema,
    table: &str,
    old_name: &str,
    new_name: &str,
) -> Result<Schema, StorageError> {
    let mut candidate = schema.clone();
    let child_relations: Vec<String> = schema
        .relations
        .iter()
        .filter(|relation| relation.child_table == table)
        .map(|relation| relation.name.clone())
        .collect();
    for definition in &mut candidate.tables {
        for column in &mut definition.columns {
            match &mut column.ty {
                ColumnType::Formula { expression } => {
                    let parsed = parse_formula(expression)?;
                    let bare = parsed.bare_columns();
                    let qualified = parsed.qualified_columns();
                    if definition.name == table && bare.iter().any(|name| name == old_name) {
                        *expression = rewrite_column_source(expression, None, old_name, new_name)?;
                    }
                    for relation in &child_relations {
                        if qualified
                            .iter()
                            .any(|(name, column)| name == relation && column == old_name)
                        {
                            *expression = rewrite_column_source(
                                expression,
                                Some(relation),
                                old_name,
                                new_name,
                            )?;
                        }
                    }
                }
                ColumnType::Lookup {
                    relation, column, ..
                } if schema
                    .relation(relation)
                    .is_some_and(|relation| relation.child_table == table)
                    && column == old_name =>
                {
                    *column = new_name.to_owned();
                }
                _ => {}
            }
            if definition.name == table && column.name == old_name {
                column.name = new_name.to_owned();
            }
        }
    }
    for relation in &mut candidate.relations {
        if relation.child_table == table && relation.child_column == old_name {
            relation.child_column = new_name.to_owned();
        }
    }
    // A rename changes identity, not form content: keep `updated_at` stable
    // so undo restores the exact previous form timestamp (the same rule as
    // `RenameTable`).
    for layout in &mut candidate.layouts {
        if layout.table == table {
            for item in &mut layout.items {
                if let LayoutItemKind::Field { column } = &mut item.kind
                    && column == old_name
                {
                    *column = new_name.to_owned();
                }
            }
        }
        for item in &mut layout.items {
            if let LayoutItemKind::Portal {
                relation, columns, ..
            } = &mut item.kind
            {
                let is_renamed_table = schema
                    .relation(relation)
                    .is_some_and(|relation| relation.child_table == table);
                if is_renamed_table {
                    for column in columns {
                        if column == old_name {
                            *column = new_name.to_owned();
                        }
                    }
                }
            }
        }
    }
    candidate.validate()?;
    FormulaDag::build(&candidate)?;
    Ok(candidate)
}

fn formula_reference(
    schema: &Schema,
    table: &str,
    column: &str,
) -> Result<Option<String>, StorageError> {
    for definition in &schema.tables {
        for formula in &definition.columns {
            match &formula.ty {
                ColumnType::Formula { expression } => {
                    let parsed = parse_formula(expression)?;
                    let bare = parsed.bare_columns();
                    if definition.name == table && bare.iter().any(|name| name == column) {
                        return Ok(Some(format!(
                            "formula {}.{}",
                            definition.name, formula.name
                        )));
                    }
                    for (relation_name, child_column) in parsed.qualified_columns() {
                        if schema
                            .relation(&relation_name)
                            .is_some_and(|relation| relation.child_table == table)
                            && child_column == column
                        {
                            return Ok(Some(format!(
                                "formula {}.{}",
                                definition.name, formula.name
                            )));
                        }
                    }
                }
                ColumnType::Lookup {
                    relation,
                    column: child_column,
                    ..
                } if schema
                    .relation(relation)
                    .is_some_and(|relation| relation.child_table == table)
                    && child_column == column =>
                {
                    return Ok(Some(format!("rollup {}.{}", definition.name, formula.name)));
                }
                _ => {}
            }
        }
    }
    Ok(None)
}

fn layout_reference(schema: &Schema, table: &str, column: &str) -> Option<String> {
    for layout in &schema.layouts {
        for item in &layout.items {
            match &item.kind {
                LayoutItemKind::Field {
                    column: item_column,
                } if layout.table == table && item_column == column => {
                    return Some(layout.name.clone());
                }
                LayoutItemKind::Portal {
                    relation, columns, ..
                } if schema
                    .relation(relation)
                    .is_some_and(|relation| relation.child_table == table)
                    && columns.iter().any(|name| name == column) =>
                {
                    return Some(layout.name.clone());
                }
                _ => {}
            }
        }
    }
    None
}

fn stored_records(
    conn: &Connection,
    table: &Table,
) -> Result<Vec<slab_core::Record>, StorageError> {
    let sql = format!(
        "SELECT {} FROM {} ORDER BY {}",
        quote_ident("id"),
        quote_ident(&table.name),
        quote_ident("id")
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt.query_map([], |row| row.get::<_, RowId>(0))?;
    let mut output = Vec::new();
    for id in ids {
        let id = id?;
        if let Some(record) = read_stored_record(conn, table, id)? {
            output.push(record);
        }
    }
    Ok(output)
}

fn stored_values(
    conn: &Connection,
    table: &Table,
    column: &Column,
) -> Result<Vec<RowValue>, StorageError> {
    if !column.ty.is_stored() {
        return Ok(Vec::new());
    }
    let sql = format!(
        "SELECT {}, {} FROM {} ORDER BY {}",
        quote_ident("id"),
        quote_ident(&column.name),
        quote_ident(&table.name),
        quote_ident("id")
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, RowId>(0)?, decode_value(row, 1, &column.ty)))
    })?;
    let mut output = Vec::new();
    for row in rows {
        let (row_id, value) = row?;
        output.push(RowValue {
            row_id,
            value: value?,
        });
    }
    Ok(output)
}

fn list_user_indexes(conn: &Connection, table: &str) -> Result<Vec<IndexSpec>, StorageError> {
    let sql = format!("PRAGMA index_list({})", quote_ident(table));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut indexes = Vec::new();
    for row in rows {
        let (name, unique, origin) = row?;
        if origin != "c" || !name.starts_with("slab_idx_") {
            continue;
        }
        let info_sql = format!("PRAGMA index_info({})", quote_ident(&name));
        let mut info = conn.prepare(&info_sql)?;
        let columns = info.query_map([], |row| row.get::<_, String>(2))?;
        let mut names = Vec::new();
        for column in columns {
            names.push(column?);
        }
        indexes.push(IndexSpec {
            name,
            table: table.to_owned(),
            columns: names,
            unique: unique != 0,
        });
    }
    Ok(indexes)
}
