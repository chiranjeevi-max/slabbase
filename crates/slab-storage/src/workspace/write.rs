use std::collections::BTreeSet;

use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use slab_core::{
    Command, DeleteAction, PolicyHook, Record, Relation, RowData, RowId, Schema, Table,
};
use slab_formula::FormulaDag;

use crate::ddl::{create_table_sql, unique_index_sql};
use crate::error::StorageError;
use crate::rollup;
use crate::sql::{encode_value, quote_ident};

use super::audit::append_audit;
use super::records::{
    capture_cascades, delete_fts, delete_row_tx, index_fts, insert_record_tx, prepare_row,
    read_computed_record, read_stored_record, rebuild_fts_for_table, refresh_fts, update_record_tx,
};
use super::schema::{
    column_definition_alter, create_index_sql, ensure_relation_values_valid, insert_column_meta,
    insert_layout_meta, insert_relation_meta, insert_table_meta, rebuild_table,
    rename_column_in_schema,
};
use super::{UNDO_LIMIT, Workspace, load_schema, now_ms};

/// Runs inside a command's transaction before audit and commit.
type PostApply<'a> = Option<&'a mut dyn FnMut(&Connection, &Command) -> Result<(), StorageError>>;

fn apply_row_update(
    tx: &Transaction<'_>,
    schema: &Schema,
    table: &Table,
    before: &Record,
    after: &Record,
) -> Result<(), StorageError> {
    update_record_tx(tx, table, after)?;
    refresh_fts(tx, table, after.id, &after.values)?;
    rollup::maintain_update(tx, schema, &table.name, before, after)
}

fn apply_row_delete(
    tx: &Transaction<'_>,
    schema: &Schema,
    policy: &dyn PolicyHook,
    table: &str,
    id: RowId,
) -> Result<Command, StorageError> {
    let definition = schema
        .table(table)
        .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
    let row = read_stored_record(tx, definition, id)?
        .ok_or_else(|| StorageError::NotFound(format!("{table} #{id}")))?;
    let mut children = Vec::new();
    let mut seen = BTreeSet::from([(table.to_owned(), id)]);
    capture_cascades(tx, schema, policy, table, id, &mut seen, &mut children)?;
    delete_row_tx(tx, table, id)?;
    delete_fts(tx, table, id)?;
    // Remove every contribution before forgetting any parent state. A
    // grandchild processed after its parent was forgotten would otherwise
    // recreate a cache row for a parent that no longer exists.
    rollup::maintain_delete(tx, schema, table, &row)?;
    for child in &children {
        delete_fts(tx, &child.table, child.row.id)?;
        rollup::maintain_delete(tx, schema, &child.table, &child.row)?;
    }
    rollup::forget_parent(tx, table, id)?;
    for child in &children {
        rollup::forget_parent(tx, &child.table, child.row.id)?;
    }
    Ok(Command::DeleteRow {
        table: table.to_owned(),
        row,
        children,
    })
}

impl Workspace {
    /// Inserts a row and returns the stored record.
    pub fn insert_row(
        &mut self,
        table: &str,
        values: &RowData,
    ) -> Result<slab_core::Record, StorageError> {
        self.ensure_writer_available()?;
        self.check_policy(table)?;
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let prepared = prepare_row(definition, values, None, true)?;
        let command = Command::InsertRow {
            table: table.to_owned(),
            row: slab_core::Record {
                id: 0,
                values: prepared,
            },
            children: Vec::new(),
        };
        self.apply_row_command_with_readback(&command, command.operation(), &command.label())
    }

    /// Inserts several rows in one transaction. Every row still receives its
    /// own audit entry and undo command, so undo semantics match single inserts.
    pub fn insert_rows(
        &mut self,
        table: &str,
        rows: &[RowData],
    ) -> Result<Vec<slab_core::Record>, StorageError> {
        self.ensure_writer_available()?;
        self.check_policy(table)?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let mut prepared = Vec::with_capacity(rows.len());
        for values in rows {
            prepared.push(prepare_row(definition, values, None, true)?);
        }
        let hlc = self.clock.now();
        let dag = FormulaDag::build(&schema)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut commands = Vec::with_capacity(prepared.len());
        let mut records = Vec::with_capacity(prepared.len());
        for values in prepared {
            let row = slab_core::Record { id: 0, values };
            let id = insert_record_tx(&tx, definition, &row, false)?;
            let applied = slab_core::Record {
                id,
                values: row.values,
            };
            index_fts(&tx, definition, id, &applied.values)?;
            rollup::maintain_insert(&tx, &schema, table, &applied)?;
            let command = Command::InsertRow {
                table: table.to_owned(),
                row: applied,
                children: Vec::new(),
            };
            append_audit(&tx, &command, &hlc, &self.actor, "insert", &command.label())?;
            let record = read_computed_record(&tx, &schema, &dag, definition, id)?;
            commands.push(command);
            records.push(record);
        }
        tx.commit()?;
        for command in &commands {
            self.push_undo(command.clone());
        }
        for record in &mut records {
            self.redact_record(definition, record);
        }
        Ok(records)
    }

    /// Starts an all-or-nothing import transaction.
    ///
    /// Rows written through [`Self::insert_import_rows`] share one SQLite
    /// transaction. Nothing is visible to other connections, and no undo entry
    /// is published, until [`Self::commit_import`] succeeds. A failed batch
    /// rolls the import back. Call [`Self::rollback_import`] if an error
    /// elsewhere, such as reading the source, prevents submitting all batches.
    pub fn begin_import(&mut self) -> Result<(), StorageError> {
        if self.pending_import.is_some() {
            return Err(StorageError::Command(
                "an import transaction is already active".into(),
            ));
        }
        self.ensure_writer_available()?;
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        self.pending_import = Some(Vec::new());
        Ok(())
    }

    /// Inserts one validated batch inside the active import transaction.
    ///
    /// Every row receives its own audit entry, matching a normal insert. A
    /// batch error rolls back the entire import, including previous batches.
    pub fn insert_import_rows(
        &mut self,
        table: &str,
        rows: &[RowData],
    ) -> Result<Vec<RowId>, StorageError> {
        let Some(mut commands) = self.pending_import.take() else {
            return Err(StorageError::Command(
                "no import transaction is active".into(),
            ));
        };
        match self.insert_import_batch(table, rows, &mut commands) {
            Ok(ids) => {
                self.pending_import = Some(commands);
                Ok(ids)
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error)
            }
        }
    }

    fn insert_import_batch(
        &mut self,
        table: &str,
        rows: &[RowData],
        commands: &mut Vec<Command>,
    ) -> Result<Vec<RowId>, StorageError> {
        self.check_policy(table)?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let schema = self.schema()?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let mut prepared = Vec::with_capacity(rows.len());
        for values in rows {
            prepared.push(prepare_row(definition, values, None, true)?);
        }
        let hlc = self.clock.now();
        let mut ids = Vec::with_capacity(prepared.len());
        for values in prepared {
            let row = slab_core::Record { id: 0, values };
            let id = insert_record_tx(&self.conn, definition, &row, false)?;
            let applied = slab_core::Record {
                id,
                values: row.values,
            };
            index_fts(&self.conn, definition, id, &applied.values)?;
            rollup::maintain_insert(&self.conn, &schema, table, &applied)?;
            let command = Command::InsertRow {
                table: table.to_owned(),
                row: applied,
                children: Vec::new(),
            };
            append_audit(
                &self.conn,
                &command,
                &hlc,
                &self.actor,
                "insert",
                &command.label(),
            )?;
            commands.push(command);
            ids.push(id);
        }
        Ok(ids)
    }

    /// Commits the active import transaction and publishes its undo entries.
    pub fn commit_import(&mut self) -> Result<(), StorageError> {
        let Some(commands) = self.pending_import.take() else {
            return Err(StorageError::Command(
                "no import transaction is active".into(),
            ));
        };
        // An import is one all-or-nothing transaction, so the same readback
        // contract as `insert_row` applies: a row whose formulas cannot be
        // evaluated must not commit.
        let validation = (|| -> Result<(), StorageError> {
            let schema = self.schema()?;
            let dag = FormulaDag::build(&schema)?;
            for command in &commands {
                let Command::InsertRow { table, row, .. } = command else {
                    continue;
                };
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                if definition
                    .columns
                    .iter()
                    .any(|column| column.ty.is_computed())
                {
                    read_computed_record(&self.conn, &schema, &dag, definition, row.id)?;
                }
            }
            Ok(())
        })();
        if let Err(error) = validation {
            let _ = self.conn.execute_batch("ROLLBACK");
            return Err(error);
        }
        match self.conn.execute_batch("COMMIT") {
            Ok(()) => {
                for command in commands {
                    self.push_undo(command);
                }
                Ok(())
            }
            Err(error) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(error.into())
            }
        }
    }

    /// Rolls back the active import transaction, discarding every row.
    pub fn rollback_import(&mut self) -> Result<(), StorageError> {
        if self.pending_import.take().is_none() {
            return Err(StorageError::Command(
                "no import transaction is active".into(),
            ));
        }
        self.conn.execute_batch("ROLLBACK")?;
        Ok(())
    }

    /// Updates a row from a partial column map and returns the stored record.
    pub fn update_row(
        &mut self,
        table: &str,
        id: RowId,
        changes: &RowData,
    ) -> Result<Record, StorageError> {
        self.ensure_writer_available()?;
        self.check_policy(table)?;
        // Another writer can commit while this connection waits for the lock.
        // Merge the patch against the row visible after acquiring it.
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let schema = load_schema(&tx)?;
        let definition = schema
            .table(table)
            .ok_or_else(|| StorageError::NotFound(table.to_owned()))?;
        let before = read_stored_record(&tx, definition, id)?
            .ok_or_else(|| StorageError::NotFound(format!("{table} #{id}")))?;
        let prepared = prepare_row(definition, changes, Some(&before.values), false)?;
        let after = Record {
            id,
            values: prepared,
        };
        let dag = FormulaDag::build(&schema)?;
        let hlc = self.clock.now();
        apply_row_update(&tx, &schema, definition, &before, &after)?;
        let mut record = read_computed_record(&tx, &schema, &dag, definition, id)?;
        let command = Command::UpdateRow {
            table: table.to_owned(),
            before,
            after,
        };
        append_audit(
            &tx,
            &command,
            &hlc,
            &self.actor,
            command.operation(),
            &command.label(),
        )?;
        tx.commit()?;
        self.push_undo(command);
        self.redact_record(definition, &mut record);
        Ok(record)
    }

    /// Deletes a row and captures cascade-deleted children for undo.
    pub fn delete_row(&mut self, table: &str, id: RowId) -> Result<(), StorageError> {
        self.ensure_writer_available()?;
        self.check_policy(table)?;
        let schema = self.schema()?;
        let hlc = self.clock.now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let command = apply_row_delete(&tx, &schema, self.policy.as_ref(), table, id)?;
        append_audit(&tx, &command, &hlc, &self.actor, "delete", &command.label())?;
        tx.commit()?;
        self.push_undo(command);
        Ok(())
    }

    fn push_undo(&mut self, command: Command) {
        self.undo.push(command);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Refuses to start a nested transaction.
    ///
    /// An import lives in one explicit `BEGIN IMMEDIATE`. If the caller
    /// abandons it, the connection stays inside that transaction and every
    /// later write fails with `cannot start a transaction within a
    /// transaction`. Rolling the abandoned transaction back here turns a
    /// permanent wedge into one recoverable, typed refusal.
    pub(super) fn ensure_writer_available(&mut self) -> Result<(), StorageError> {
        if self.pending_import.take().is_some() || !self.conn.is_autocommit() {
            let _ = self.conn.execute_batch("ROLLBACK");
            return Err(StorageError::Command(
                "an abandoned transaction was rolled back; retry the operation".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn next_id(&mut self, prefix: &str) -> String {
        self.id_seed = self.id_seed.wrapping_add(1);
        format!("{prefix}_{:x}_{:x}", now_ms(), self.id_seed)
    }

    pub(super) fn apply_command(&mut self, command: &Command) -> Result<Command, StorageError> {
        let applied =
            self.apply_command_with_audit(command, command.operation(), &command.label())?;
        self.push_undo(applied.clone());
        Ok(applied)
    }

    /// Checks the replay target and any rows an inverse insert will restore.
    /// A delete authorizes its live cascade in the writer transaction.
    pub(super) fn check_command_policy(&self, command: &Command) -> Result<(), StorageError> {
        if let Some(table) = command.table_name() {
            self.check_policy(table)?;
        }
        if let Command::InsertRow { children, .. } = command {
            for child in children {
                self.check_policy(&child.table)?;
            }
        }
        Ok(())
    }

    /// Applies a row mutation and returns its fully computed record from the
    /// same transaction.
    ///
    /// Reading the record back after `COMMIT` turned a committed insert into a
    /// reported failure whenever a formula could not be evaluated (for example
    /// `[A] / [B]` with `B = 0`). The read now happens before the commit, so a
    /// failed read rolls the row, its audit entry, and its FTS/rollup updates
    /// back together.
    pub(super) fn apply_row_command_with_readback(
        &mut self,
        command: &Command,
        operation: &str,
        summary: &str,
    ) -> Result<slab_core::Record, StorageError> {
        let table_name = match command {
            Command::InsertRow { table, .. } | Command::UpdateRow { table, .. } => table,
            _ => {
                return Err(StorageError::Internal(
                    "row readback received a non-row command".into(),
                ));
            }
        };
        // The record returned by a write is a read. Apply the same table policy
        // gate before the mutation so a denied table fails without committing.
        self.check_policy(table_name)?;
        let schema = self.schema()?;
        let definition = schema
            .table(table_name)
            .ok_or_else(|| StorageError::NotFound(table_name.to_owned()))?;
        let dag = FormulaDag::build(&schema)?;
        let mut computed: Option<slab_core::Record> = None;
        let applied = {
            let mut readback = |conn: &Connection, applied: &Command| -> Result<(), StorageError> {
                let (table_name, row_id) = match applied {
                    Command::InsertRow { table, row, .. } => (table.as_str(), row.id),
                    Command::UpdateRow { table, after, .. } => (table.as_str(), after.id),
                    _ => {
                        return Err(StorageError::Internal(
                            "row readback received a non-row command".into(),
                        ));
                    }
                };
                let definition = schema
                    .table(table_name)
                    .ok_or_else(|| StorageError::NotFound(table_name.to_owned()))?;
                computed = Some(read_computed_record(
                    conn, &schema, &dag, definition, row_id,
                )?);
                Ok(())
            };
            self.apply_command_tx(command, operation, summary, Some(&mut readback))?
        };
        let mut record = computed.ok_or_else(|| {
            StorageError::Internal("row readback did not produce a record".into())
        })?;
        self.push_undo(applied);
        self.redact_record(definition, &mut record);
        Ok(record)
    }

    pub(super) fn apply_command_with_audit(
        &mut self,
        command: &Command,
        operation: &str,
        summary: &str,
    ) -> Result<Command, StorageError> {
        match command {
            // These commands rebuild a table with the drop-and-rename dance.
            // With foreign keys enabled, `DROP TABLE` performs an implicit
            // delete and fires ON DELETE CASCADE against every child row. The
            // final `pragma_foreign_key_check` inside `rebuild_table` validates
            // the result, so enforcement is suspended only for the rebuild.
            Command::CreateRelation { .. }
            | Command::DropRelation { .. }
            | Command::DropColumn { .. }
            | Command::RestoreColumn { .. } => self.with_foreign_keys_off(|workspace| {
                workspace.apply_command_tx(command, operation, summary, None)
            }),
            _ => self.apply_command_tx(command, operation, summary, None),
        }
    }

    fn with_foreign_keys_off<T>(
        &mut self,
        work: impl FnOnce(&mut Self) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        self.conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
        let outcome = work(self);
        let reenabled = self.conn.execute_batch("PRAGMA foreign_keys = ON;");
        match (outcome, reenabled) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error.into()),
        }
    }

    fn apply_command_tx(
        &mut self,
        command: &Command,
        operation: &str,
        summary: &str,
        post_apply: PostApply<'_>,
    ) -> Result<Command, StorageError> {
        self.ensure_writer_available()?;
        let schema = self.schema()?;
        let hlc = self.clock.now();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let applied = match command {
            Command::InsertRow {
                table,
                row,
                children,
            } => {
                if !children.is_empty() {
                    // A captured descendant can reference another row later
                    // in the cascade. Validate all references at commit.
                    tx.pragma_update(None, "defer_foreign_keys", true)?;
                }
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                let id = insert_record_tx(&tx, definition, row, row.id > 0)?;
                let mut applied_row = row.clone();
                applied_row.id = id;
                index_fts(&tx, definition, id, &applied_row.values)?;
                rollup::maintain_insert(&tx, &schema, table, &applied_row)?;
                for child in children {
                    let child_table = schema
                        .table(&child.table)
                        .ok_or_else(|| StorageError::NotFound(child.table.clone()))?;
                    insert_record_tx(&tx, child_table, &child.row, true)?;
                    index_fts(&tx, child_table, child.row.id, &child.row.values)?;
                    rollup::maintain_insert(&tx, &schema, &child.table, &child.row)?;
                }
                Command::InsertRow {
                    table: table.clone(),
                    row: applied_row,
                    children: children.clone(),
                }
            }
            Command::UpdateRow {
                table,
                before,
                after,
            } => {
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                apply_row_update(&tx, &schema, definition, before, after)?;
                command.clone()
            }
            Command::DeleteRow { table, row, .. } => {
                apply_row_delete(&tx, &schema, self.policy.as_ref(), table, row.id)?
            }
            Command::CreateTable { table } => {
                let sql = create_table_sql(table, &table.name, &[])?;
                tx.execute_batch(&sql)?;
                insert_table_meta(&tx, table)?;
                command.clone()
            }
            Command::DropTable { table, .. } => {
                tx.execute_batch(&format!("DROP TABLE {}", quote_ident(&table.name)))?;
                tx.execute(
                    "DELETE FROM _slab_tables WHERE name = ?1",
                    params![table.name],
                )?;
                tx.execute(
                    "DELETE FROM _slab_fts WHERE table_name = ?1",
                    params![table.name],
                )?;
                command.clone()
            }
            Command::RestoreTable {
                table,
                rows,
                layouts,
                indexes,
            } => {
                let sql = create_table_sql(table, &table.name, &[])?;
                tx.execute_batch(&sql)?;
                insert_table_meta(&tx, table)?;
                for layout in layouts {
                    insert_layout_meta(&tx, layout)?;
                }
                for row in rows {
                    insert_record_tx(&tx, table, row, true)?;
                    index_fts(&tx, table, row.id, &row.values)?;
                }
                for index in indexes {
                    tx.execute_batch(&create_index_sql(index))?;
                }
                command.clone()
            }
            Command::RenameTable { old_name, new_name } => {
                tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
                tx.execute_batch(&format!(
                    "ALTER TABLE {} RENAME TO {}",
                    quote_ident(old_name),
                    quote_ident(new_name)
                ))?;
                tx.execute(
                    "UPDATE _slab_tables SET name = ?1 WHERE name = ?2",
                    params![new_name, old_name],
                )?;
                tx.execute(
                    "UPDATE _slab_columns SET table_name = ?1 WHERE table_name = ?2",
                    params![new_name, old_name],
                )?;
                tx.execute(
                    "UPDATE _slab_relations SET parent_table = ?1 WHERE parent_table = ?2",
                    params![new_name, old_name],
                )?;
                tx.execute(
                    "UPDATE _slab_relations SET child_table = ?1 WHERE child_table = ?2",
                    params![new_name, old_name],
                )?;
                for layout in &schema.layouts {
                    if layout.table == *old_name {
                        let mut updated = layout.clone();
                        updated.table = new_name.clone();
                        // A rename changes identity, not form content, so the
                        // original timestamp survives undo/redo exactly.
                        tx.execute(
                            "UPDATE _slab_layouts SET table_name = ?1, spec_json = ?2
                                WHERE id = ?3",
                            params![new_name, serde_json::to_string(&updated)?, updated.id],
                        )?;
                    }
                }
                tx.execute(
                    "UPDATE _slab_fts SET table_name = ?1 WHERE table_name = ?2",
                    params![new_name, old_name],
                )?;
                let violations: i64 =
                    tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| {
                        row.get(0)
                    })?;
                if violations > 0 {
                    return Err(StorageError::ForeignKey(format!(
                        "{violations} rows violate foreign keys after renaming {old_name}"
                    )));
                }
                command.clone()
            }
            Command::RenameColumn {
                table,
                old_name,
                new_name,
            } => {
                let candidate = rename_column_in_schema(&schema, table, old_name, new_name)?;
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                if definition
                    .column(old_name)
                    .is_some_and(|column| column.ty.is_stored())
                {
                    tx.execute_batch(&format!(
                        "ALTER TABLE {} RENAME COLUMN {} TO {}",
                        quote_ident(table),
                        quote_ident(old_name),
                        quote_ident(new_name)
                    ))?;
                }
                for definition in &candidate.tables {
                    for column in &definition.columns {
                        tx.execute(
                            "UPDATE _slab_columns SET name = ?1, type_json = ?2 WHERE id = ?3",
                            params![column.name, serde_json::to_string(&column.ty)?, column.id],
                        )?;
                    }
                }
                for relation in &candidate.relations {
                    tx.execute(
                        "UPDATE _slab_relations SET parent_table = ?1, child_table = ?2,
                            child_column = ?3, on_delete = ?4 WHERE id = ?5",
                        params![
                            relation.parent_table,
                            relation.child_table,
                            relation.child_column,
                            match relation.on_delete {
                                DeleteAction::Cascade => "cascade",
                                DeleteAction::SetNull => "setNull",
                                DeleteAction::Restrict => "restrict",
                            },
                            relation.id,
                        ],
                    )?;
                }
                for layout in &candidate.layouts {
                    tx.execute(
                        "UPDATE _slab_layouts SET name = ?1, table_name = ?2, spec_json = ?3,
                            updated_at = ?4 WHERE id = ?5",
                        params![
                            layout.name,
                            layout.table,
                            serde_json::to_string(layout)?,
                            layout.updated_at,
                            layout.id
                        ],
                    )?;
                }
                command.clone()
            }
            Command::AddColumn { table, column } => {
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                if column.ty.is_stored() {
                    let exists: i64 = tx.query_row(
                        &format!("SELECT COUNT(*) FROM {}", quote_ident(table)),
                        [],
                        |row| row.get(0),
                    )?;
                    if column.required && column.default.is_none() && exists > 0 {
                        return Err(StorageError::Command(format!(
                            "{table}.{} cannot be required without a default on a non-empty table",
                            column.name
                        )));
                    }
                    let ddl = column_definition_alter(column)?;
                    tx.execute_batch(&format!(
                        "ALTER TABLE {} ADD COLUMN {ddl}",
                        quote_ident(table)
                    ))?;
                    if column.unique {
                        tx.execute_batch(&unique_index_sql(table, &column.name))?;
                    }
                    let mut updated = definition.clone();
                    updated.columns.push(column.clone());
                    rebuild_fts_for_table(&tx, &updated)?;
                }
                insert_column_meta(&tx, column)?;
                command.clone()
            }
            Command::DropColumn {
                table,
                column,
                values: _,
            } => {
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                if schema.relations.iter().any(|relation| {
                    relation.child_table == *table && relation.child_column == column.name
                }) {
                    return Err(StorageError::Command(format!(
                        "column {table}.{} is used by a relation",
                        column.name
                    )));
                }
                let relations = schema.relations.clone();
                let mut reduced = definition.clone();
                reduced
                    .columns
                    .retain(|existing| existing.name != column.name);
                rebuild_table(&tx, &reduced, &relations)?;
                tx.execute(
                    "DELETE FROM _slab_columns WHERE table_name = ?1 AND name = ?2",
                    params![table, column.name],
                )?;
                rebuild_fts_for_table(&tx, &reduced)?;
                command.clone()
            }
            Command::RestoreColumn {
                table,
                column,
                values,
            } => {
                let definition = schema
                    .table(table)
                    .ok_or_else(|| StorageError::NotFound(table.clone()))?;
                let mut expanded = definition.clone();
                expanded.columns.push(column.clone());
                expanded.columns.sort_by_key(|candidate| candidate.position);
                let mut staged = expanded.clone();
                if let Some(staged_column) = staged.column_mut(&column.name) {
                    staged_column.required = false;
                }
                rebuild_table(&tx, &staged, &schema.relations)?;
                insert_column_meta(&tx, column)?;
                for row_value in values {
                    tx.execute(
                        &format!(
                            "UPDATE {} SET {} = ?1 WHERE {} = ?2",
                            quote_ident(table),
                            quote_ident(&column.name),
                            quote_ident("id")
                        ),
                        params![
                            encode_value(&row_value.value, &column.ty)?,
                            row_value.row_id
                        ],
                    )?;
                }
                if column.required && column.ty.is_stored() {
                    rebuild_table(&tx, &expanded, &schema.relations)?;
                }
                rebuild_fts_for_table(&tx, &expanded)?;
                command.clone()
            }
            Command::CreateIndex { index } => {
                tx.execute_batch(&create_index_sql(index))?;
                command.clone()
            }
            Command::DropIndex { index } => {
                tx.execute_batch(&format!("DROP INDEX {}", quote_ident(&index.name)))?;
                command.clone()
            }
            Command::CreateRelation { relation } => {
                let child = schema
                    .table(&relation.child_table)
                    .ok_or_else(|| StorageError::NotFound(relation.child_table.clone()))?;
                let mut relations = schema.relations.clone();
                relations.push(relation.clone());
                ensure_relation_values_valid(
                    &tx,
                    &relation.child_table,
                    &relation.child_column,
                    &relation.parent_table,
                )?;
                rebuild_table(&tx, child, &relations)?;
                insert_relation_meta(&tx, relation)?;
                command.clone()
            }
            Command::DropRelation { relation } => {
                let child = schema
                    .table(&relation.child_table)
                    .ok_or_else(|| StorageError::NotFound(relation.child_table.clone()))?;
                tx.execute(
                    "DELETE FROM _slab_relations WHERE id = ?1",
                    params![relation.id],
                )?;
                let relations: Vec<Relation> = schema
                    .relations
                    .iter()
                    .filter(|existing| existing.id != relation.id)
                    .cloned()
                    .collect();
                rebuild_table(&tx, child, &relations)?;
                command.clone()
            }
            Command::SaveLayout { before, after } => {
                let before = before.clone();
                tx.execute(
                    "INSERT INTO _slab_layouts (id, name, table_name, spec_json, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(id) DO UPDATE SET
                        name = excluded.name,
                        table_name = excluded.table_name,
                        spec_json = excluded.spec_json,
                        updated_at = excluded.updated_at",
                    params![
                        after.id,
                        after.name,
                        after.table,
                        serde_json::to_string(after)?,
                        after.updated_at
                    ],
                )?;
                Command::SaveLayout {
                    before,
                    after: after.clone(),
                }
            }
            Command::DeleteLayout { layout } => {
                tx.execute(
                    "DELETE FROM _slab_layouts WHERE id = ?1",
                    params![layout.id],
                )?;
                command.clone()
            }
        };
        if let Some(post_apply) = post_apply {
            post_apply(&tx, &applied)?;
        }
        let audit_summary = match (command, &applied) {
            // A fresh insert enters the transaction without an id; the summary
            // must name the row SQLite actually assigned, not `#0`.
            (Command::InsertRow { row, .. }, Command::InsertRow { .. }) if row.id == 0 => {
                applied.label()
            }
            _ => summary.to_owned(),
        };
        append_audit(&tx, &applied, &hlc, &self.actor, operation, &audit_summary)?;
        let schema_change = command.is_schema_change();
        if schema_change {
            rollup::clear(&tx)?;
        }
        tx.commit()?;
        if schema_change {
            self.refresh_readers();
        }
        Ok(applied)
    }
}
