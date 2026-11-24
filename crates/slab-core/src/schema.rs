//! Schema definitions for user tables, columns, relations, and indexes.

use serde::{Deserialize, Serialize};

use crate::decimal::{CurrencyCode, MAX_SCALE};
use crate::error::{CoreError, validate_identifier};
use crate::layout::Layout;
use crate::value::Value;

/// An aggregate applied over a related child column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub enum Aggregate {
    /// Count of non-null child values.
    Count,
    /// Sum of child values.
    Sum,
    /// Arithmetic mean of child values.
    Avg,
    /// Minimum child value.
    Min,
    /// Maximum child value.
    Max,
}

impl Aggregate {
    /// The stable lowercase label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Avg => "avg",
            Self::Min => "min",
            Self::Max => "max",
        }
    }
}

/// The type and configuration of a column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ColumnType {
    /// UTF-8 text.
    Text,
    /// 64-bit integer.
    Integer,
    /// Exact decimal with a fixed scale.
    Decimal {
        /// Fractional digits.
        scale: u8,
    },
    /// Exact money in one currency.
    Currency {
        /// ISO 4217 style code.
        #[cfg_attr(feature = "ts", ts(type = "string"))]
        currency: CurrencyCode,
    },
    /// Boolean.
    Boolean,
    /// Calendar date.
    Date,
    /// UTC timestamp.
    Timestamp,
    /// A value from a fixed option list.
    Select {
        /// Allowed options.
        options: Vec<String>,
    },
    /// A reference to an external attachment.
    AttachmentRef,
    /// A computed expression.
    Formula {
        /// Formula source text.
        expression: String,
    },
    /// A computed aggregate over a child relation.
    Lookup {
        /// Relation name.
        relation: String,
        /// Child column name.
        column: String,
        /// Aggregate function.
        aggregate: Aggregate,
    },
}

impl ColumnType {
    /// The stable lowercase type name.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Integer => "integer",
            Self::Decimal { .. } => "decimal",
            Self::Currency { .. } => "currency",
            Self::Boolean => "boolean",
            Self::Date => "date",
            Self::Timestamp => "timestamp",
            Self::Select { .. } => "select",
            Self::AttachmentRef => "attachment",
            Self::Formula { .. } => "formula",
            Self::Lookup { .. } => "lookup",
        }
    }

    /// True when values are written to a physical column.
    pub const fn is_stored(&self) -> bool {
        !matches!(self, Self::Formula { .. } | Self::Lookup { .. })
    }

    /// True when the value is computed on read.
    pub const fn is_computed(&self) -> bool {
        !self.is_stored()
    }

    /// Validates embedded configuration.
    pub fn validate(&self, table: &str, column: &str) -> Result<(), CoreError> {
        match self {
            Self::Decimal { scale } if *scale > MAX_SCALE => Err(CoreError::InvalidColumn {
                reason: format!("{table}.{column}: decimal scale {scale} exceeds {MAX_SCALE}"),
            }),
            Self::Select { options } => {
                if options.is_empty() {
                    return Err(CoreError::InvalidColumn {
                        reason: format!("{table}.{column}: select has no options"),
                    });
                }
                let mut seen = std::collections::BTreeSet::new();
                for option in options {
                    if option.trim().is_empty() {
                        return Err(CoreError::InvalidColumn {
                            reason: format!("{table}.{column}: empty select option"),
                        });
                    }
                    if !seen.insert(option) {
                        return Err(CoreError::InvalidColumn {
                            reason: format!("{table}.{column}: duplicate select option {option:?}"),
                        });
                    }
                }
                Ok(())
            }
            Self::Formula { expression } if expression.trim().is_empty() => {
                Err(CoreError::InvalidColumn {
                    reason: format!("{table}.{column}: formula is empty"),
                })
            }
            Self::Lookup {
                relation,
                column: child,
                ..
            } => {
                validate_identifier(relation).map_err(|_| CoreError::InvalidColumn {
                    reason: format!("{table}.{column}: invalid relation name {relation:?}"),
                })?;
                validate_identifier(child).map_err(|_| CoreError::InvalidColumn {
                    reason: format!("{table}.{column}: invalid lookup column {child:?}"),
                })?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Checks whether a value can be written to this column.
    pub fn accepts(&self, value: &Value) -> bool {
        if value.is_null() {
            return true;
        }
        match (self, value) {
            (Self::Text, Value::Text(_))
            | (Self::Integer, Value::Integer(_))
            | (Self::Boolean, Value::Boolean(_))
            | (Self::Date, Value::Date(_))
            | (Self::Timestamp, Value::Timestamp(_))
            | (Self::AttachmentRef, Value::AttachmentRef(_)) => true,
            (Self::Decimal { .. }, Value::Decimal(_) | Value::Integer(_)) => true,
            (Self::Currency { currency }, Value::Currency(money)) => money.currency() == *currency,
            (Self::Select { options }, Value::Select(selected)) => {
                options.iter().any(|option| option == selected)
            }
            (Self::Formula { .. }, _) | (Self::Lookup { .. }, _) => false,
            _ => false,
        }
    }
}

/// A user column definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct Column {
    /// Stable identifier.
    pub id: String,
    /// Owning table name.
    pub table: String,
    /// Column name, unique within the table.
    pub name: String,
    /// Type and configuration.
    pub ty: ColumnType,
    /// Whether null is rejected.
    pub required: bool,
    /// Whether values must be unique.
    pub unique: bool,
    /// Optional default applied on insert.
    pub default: Option<Value>,
    /// Display order.
    pub position: u32,
}

impl Column {
    /// Validates the name, embedded type configuration, and default value.
    pub fn validate(&self) -> Result<(), CoreError> {
        validate_identifier(&self.name)?;
        self.ty.validate(&self.table, &self.name)?;
        if let Some(default) = &self.default {
            if !self.ty.accepts(default) {
                return Err(CoreError::InvalidColumn {
                    reason: format!(
                        "{}.{}: default value is {} but column is {}",
                        self.table,
                        self.name,
                        default.type_name(),
                        self.ty.name()
                    ),
                });
            }
            if default.is_null() && self.required {
                return Err(CoreError::InvalidColumn {
                    reason: format!(
                        "{}.{}: required column cannot default to null",
                        self.table, self.name
                    ),
                });
            }
        }
        if self.ty.is_computed() && self.unique {
            return Err(CoreError::InvalidColumn {
                reason: format!(
                    "{}.{}: computed columns cannot be unique",
                    self.table, self.name
                ),
            });
        }
        Ok(())
    }
}

/// What happens to child rows when a parent row is deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
#[serde(rename_all = "camelCase")]
pub enum DeleteAction {
    /// Delete child rows.
    Cascade,
    /// Set the child foreign key to null.
    SetNull,
    /// Reject the parent delete.
    Restrict,
}

impl DeleteAction {
    /// SQL clause for the foreign key.
    pub const fn sql(self) -> &'static str {
        match self {
            Self::Cascade => "CASCADE",
            Self::SetNull => "SET NULL",
            Self::Restrict => "RESTRICT",
        }
    }
}

/// A parent-to-child foreign key relation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct Relation {
    /// Stable identifier.
    pub id: String,
    /// Name used in formulas and portals.
    pub name: String,
    /// Parent table name.
    pub parent_table: String,
    /// Child table name.
    pub child_table: String,
    /// Child column holding the parent id.
    pub child_column: String,
    /// Delete behavior.
    pub on_delete: DeleteAction,
}

impl Relation {
    /// Validates names and basic shape.
    pub fn validate(&self) -> Result<(), CoreError> {
        validate_identifier(&self.name)?;
        if self.parent_table == self.child_table {
            return Err(CoreError::InvalidColumn {
                reason: format!("relation {}: self relations are not supported", self.name),
            });
        }
        Ok(())
    }
}

/// A user table definition.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct Table {
    /// Stable identifier.
    pub id: String,
    /// Table name, unique in the workspace.
    pub name: String,
    /// Ordered columns, excluding the implicit `id` primary key.
    pub columns: Vec<Column>,
    /// Creation time in Unix milliseconds.
    pub created_at: i64,
}

impl Table {
    /// Finds a column by name.
    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|column| column.name == name)
    }

    /// Finds a column by name with mutable access.
    pub fn column_mut(&mut self, name: &str) -> Option<&mut Column> {
        self.columns.iter_mut().find(|column| column.name == name)
    }

    /// Columns written to physical storage.
    pub fn stored_columns(&self) -> impl Iterator<Item = &Column> {
        self.columns.iter().filter(|column| column.ty.is_stored())
    }

    /// Validates the table and all columns.
    pub fn validate(&self) -> Result<(), CoreError> {
        validate_identifier(&self.name)?;
        let mut names = std::collections::BTreeSet::new();
        for column in &self.columns {
            column.validate()?;
            if !names.insert(&column.name) {
                return Err(CoreError::InvalidColumn {
                    reason: format!("{}: duplicate column {}", self.name, column.name),
                });
            }
        }
        Ok(())
    }
}

/// A secondary index over stored columns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct IndexSpec {
    /// Index name.
    pub name: String,
    /// Table name.
    pub table: String,
    /// Indexed columns, in order.
    pub columns: Vec<String>,
    /// Whether the index is unique.
    pub unique: bool,
}

/// A column supplied by the shell when creating a table or adding a column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct ColumnDraft {
    /// Column name.
    pub name: String,
    /// Type and configuration.
    pub ty: ColumnType,
    /// Whether null is rejected.
    pub required: bool,
    /// Whether values must be unique.
    pub unique: bool,
    /// Optional default applied on insert.
    pub default: Option<Value>,
}

/// A table supplied by the shell when creating a table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct TableDraft {
    /// Table name.
    pub name: String,
    /// Ordered columns, excluding the implicit id.
    pub columns: Vec<ColumnDraft>,
}

/// A relation supplied by the shell when linking two tables.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct RelationDraft {
    /// Relation name, unique in the workspace.
    pub name: String,
    /// Parent table.
    pub parent_table: String,
    /// Child table.
    pub child_table: String,
    /// Child column holding the parent id.
    pub child_column: String,
    /// Delete behavior.
    pub on_delete: DeleteAction,
}

/// The complete workspace schema.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(
    feature = "ts",
    ts(export, export_to = "../../../apps/desktop/src/bindings/")
)]
pub struct Schema {
    /// User tables.
    pub tables: Vec<Table>,
    /// Relations between tables.
    pub relations: Vec<Relation>,
    /// Saved form layouts.
    pub layouts: Vec<Layout>,
}

impl Schema {
    /// Finds a table by name.
    pub fn table(&self, name: &str) -> Option<&Table> {
        self.tables.iter().find(|table| table.name == name)
    }

    /// Finds a table by name with mutable access.
    pub fn table_mut(&mut self, name: &str) -> Option<&mut Table> {
        self.tables.iter_mut().find(|table| table.name == name)
    }

    /// Finds a relation by name.
    pub fn relation(&self, name: &str) -> Option<&Relation> {
        self.relations.iter().find(|relation| relation.name == name)
    }

    /// Relations where the given table is the parent.
    pub fn child_relations<'a>(&'a self, table: &'a str) -> impl Iterator<Item = &'a Relation> {
        self.relations
            .iter()
            .filter(move |relation| relation.parent_table == table)
    }

    /// Relations where the given table is the child.
    pub fn parent_relations<'a>(&'a self, table: &'a str) -> impl Iterator<Item = &'a Relation> {
        self.relations
            .iter()
            .filter(move |relation| relation.child_table == table)
    }

    /// Validates the whole schema, including relations and layouts.
    pub fn validate(&self) -> Result<(), CoreError> {
        let mut table_names = std::collections::BTreeSet::new();
        for table in &self.tables {
            table.validate()?;
            if !table_names.insert(table.name.as_str()) {
                return Err(CoreError::InvalidColumn {
                    reason: format!("duplicate table {}", table.name),
                });
            }
        }
        let mut relation_names = std::collections::BTreeSet::new();
        for relation in &self.relations {
            relation.validate()?;
            if !relation_names.insert(relation.name.as_str()) {
                return Err(CoreError::InvalidColumn {
                    reason: format!("duplicate relation {}", relation.name),
                });
            }
            let parent =
                self.table(&relation.parent_table)
                    .ok_or_else(|| CoreError::InvalidColumn {
                        reason: format!(
                            "relation {}: missing parent table {}",
                            relation.name, relation.parent_table
                        ),
                    })?;
            let child =
                self.table(&relation.child_table)
                    .ok_or_else(|| CoreError::InvalidColumn {
                        reason: format!(
                            "relation {}: missing child table {}",
                            relation.name, relation.child_table
                        ),
                    })?;
            if parent.column(&relation.child_column).is_some() {
                return Err(CoreError::InvalidColumn {
                    reason: format!(
                        "relation {}: column {} already exists on parent {}",
                        relation.name, relation.child_column, relation.parent_table
                    ),
                });
            }
            let Some(column) = child.column(&relation.child_column) else {
                return Err(CoreError::InvalidColumn {
                    reason: format!(
                        "relation {}: missing child column {}.{}",
                        relation.name, relation.child_table, relation.child_column
                    ),
                });
            };
            if !column.ty.is_stored() {
                return Err(CoreError::InvalidColumn {
                    reason: format!("relation {}: child column must be stored", relation.name),
                });
            }
        }
        for table in &self.tables {
            for column in &table.columns {
                if let ColumnType::Lookup {
                    relation,
                    column: child_column,
                    ..
                } = &column.ty
                {
                    let lookup =
                        self.relation(relation)
                            .ok_or_else(|| CoreError::InvalidColumn {
                                reason: format!(
                                    "{}.{}: lookup relation {relation} does not exist",
                                    table.name, column.name
                                ),
                            })?;
                    if lookup.parent_table != table.name {
                        return Err(CoreError::InvalidColumn {
                            reason: format!(
                                "{}.{}: lookup relation {} belongs to {}",
                                table.name, column.name, relation, lookup.parent_table
                            ),
                        });
                    }
                    let child = self.table(&lookup.child_table).ok_or_else(|| {
                        CoreError::InvalidColumn {
                            reason: format!(
                                "{}.{}: lookup child table is missing",
                                table.name, column.name
                            ),
                        }
                    })?;
                    if child.column(child_column).is_none() {
                        return Err(CoreError::InvalidColumn {
                            reason: format!(
                                "{}.{}: lookup child column {child_column} is missing",
                                table.name, column.name
                            ),
                        });
                    }
                }
            }
            let mut stack = vec![table.name.clone()];
            let mut visited = std::collections::BTreeSet::new();
            while let Some(current) = stack.pop() {
                if !visited.insert(current.clone()) {
                    continue;
                }
                for relation in self
                    .relations
                    .iter()
                    .filter(|relation| relation.parent_table == current)
                {
                    if relation.child_table == table.name {
                        return Err(CoreError::InvalidColumn {
                            reason: format!(
                                "relation cycle involving {} and {}",
                                table.name, relation.child_table
                            ),
                        });
                    }
                    stack.push(relation.child_table.clone());
                }
            }
        }
        for layout in &self.layouts {
            layout.validate(self)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(name: &str, columns: &[&str]) -> Table {
        Table {
            id: format!("t_{name}"),
            name: name.to_owned(),
            columns: columns
                .iter()
                .enumerate()
                .map(|(index, name)| Column {
                    id: format!("c_{name}"),
                    table: table_name(name),
                    name: (*name).to_owned(),
                    ty: ColumnType::Text,
                    required: false,
                    unique: false,
                    default: None,
                    position: u32::try_from(index).unwrap_or(0),
                })
                .collect(),
            created_at: 0,
        }
    }

    fn table_name(column: &str) -> String {
        format!("t_{column}")
    }

    #[test]
    fn rejects_duplicate_columns() {
        let mut table = table("Clients", &["Name"]);
        table.columns.push(table.columns[0].clone());
        assert!(table.validate().is_err());
    }

    #[test]
    fn rejects_relation_cycles() {
        let clients = Table {
            id: "t_clients".into(),
            name: "Clients".into(),
            columns: vec![Column {
                id: "c_client".into(),
                table: "Clients".into(),
                name: "Name".into(),
                ty: ColumnType::Text,
                required: false,
                unique: false,
                default: None,
                position: 0,
            }],
            created_at: 0,
        };
        let projects = Table {
            id: "t_projects".into(),
            name: "Projects".into(),
            columns: vec![Column {
                id: "c_project".into(),
                table: "Projects".into(),
                name: "Client".into(),
                ty: ColumnType::Integer,
                required: false,
                unique: false,
                default: None,
                position: 0,
            }],
            created_at: 0,
        };
        let relation = |name: &str, parent: &str, child: &str, column: &str| Relation {
            id: format!("r_{name}"),
            name: name.into(),
            parent_table: parent.into(),
            child_table: child.into(),
            child_column: column.into(),
            on_delete: DeleteAction::Cascade,
        };
        let schema = Schema {
            tables: vec![clients, projects],
            relations: vec![
                relation("Client Projects", "Clients", "Projects", "Client"),
                relation("Project Clients", "Projects", "Clients", "Name"),
            ],
            layouts: Vec::new(),
        };
        assert!(schema.validate().is_err());
    }

    #[test]
    fn defaults_must_match_column_type() {
        let column = Column {
            id: "c".into(),
            table: "Clients".into(),
            name: "Active".into(),
            ty: ColumnType::Boolean,
            required: false,
            unique: false,
            default: Some(Value::Text("yes".into())),
            position: 0,
        };
        assert!(column.validate().is_err());
    }
}
