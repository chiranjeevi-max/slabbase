//! Static types for formula results and the column environment.

use std::collections::BTreeMap;

use slab_core::{Aggregate, ColumnType, Schema, Value};

use crate::ast::{BinaryOp, Expr, UnaryOp};
use crate::error::FormulaError;
use crate::parser;

/// A formula's result type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormulaType {
    /// Not yet known, for a computed column that has not been resolved.
    Any,
    /// Text.
    Text,
    /// Integer.
    Integer,
    /// Exact decimal.
    Decimal,
    /// Exact money.
    Currency,
    /// Boolean.
    Boolean,
    /// Calendar date.
    Date,
    /// UTC timestamp.
    Timestamp,
}

impl FormulaType {
    /// A lowercase name used in error messages and IPC.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::Text => "text",
            Self::Integer => "integer",
            Self::Decimal => "decimal",
            Self::Currency => "currency",
            Self::Boolean => "boolean",
            Self::Date => "date",
            Self::Timestamp => "timestamp",
        }
    }

    /// True for integer and decimal types.
    pub const fn is_numeric(self) -> bool {
        matches!(self, Self::Integer | Self::Decimal)
    }

    /// True for date and timestamp types.
    pub const fn is_temporal(self) -> bool {
        matches!(self, Self::Date | Self::Timestamp)
    }

    /// Maps a stored column type to a formula type.
    pub fn from_column_type(column_type: &ColumnType) -> Self {
        match column_type {
            ColumnType::Text | ColumnType::Select { .. } | ColumnType::AttachmentRef => Self::Text,
            ColumnType::Integer => Self::Integer,
            ColumnType::Decimal { .. } => Self::Decimal,
            ColumnType::Currency { .. } => Self::Currency,
            ColumnType::Boolean => Self::Boolean,
            ColumnType::Date => Self::Date,
            ColumnType::Timestamp => Self::Timestamp,
            ColumnType::Formula { .. } | ColumnType::Lookup { .. } => Self::Any,
        }
    }

    /// Maps a runtime value to a formula type.
    pub fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::Any,
            Value::Text(_) | Value::Select(_) | Value::AttachmentRef(_) => Self::Text,
            Value::Integer(_) => Self::Integer,
            Value::Decimal(_) => Self::Decimal,
            Value::Currency(_) => Self::Currency,
            Value::Boolean(_) => Self::Boolean,
            Value::Date(_) => Self::Date,
            Value::Timestamp(_) => Self::Timestamp,
        }
    }
}

/// Column and relation types for one table.
#[derive(Clone, Debug, Default)]
pub struct TypeEnv {
    columns: BTreeMap<String, FormulaType>,
    relations: BTreeMap<String, BTreeMap<String, FormulaType>>,
}

impl TypeEnv {
    /// Builds the environment for a table in a schema. Computed columns are
    /// typed as `Any` until the dependency graph resolves them.
    pub fn for_table(schema: &Schema, table_name: &str) -> Result<Self, FormulaError> {
        let table = schema
            .table(table_name)
            .ok_or_else(|| FormulaError::UnknownTable(table_name.to_owned()))?;
        let mut env = Self::default();
        env.columns.insert("id".into(), FormulaType::Integer);
        for column in &table.columns {
            env.columns.insert(
                column.name.clone(),
                FormulaType::from_column_type(&column.ty),
            );
        }
        for relation in schema.child_relations(table_name) {
            let Some(child) = schema.table(&relation.child_table) else {
                continue;
            };
            let mut child_columns = BTreeMap::new();
            child_columns.insert("id".into(), FormulaType::Integer);
            for column in &child.columns {
                child_columns.insert(
                    column.name.clone(),
                    FormulaType::from_column_type(&column.ty),
                );
            }
            env.relations.insert(relation.name.clone(), child_columns);
        }
        Ok(env)
    }

    /// Adds or replaces a column type.
    pub fn with_column(mut self, name: impl Into<String>, ty: FormulaType) -> Self {
        self.columns.insert(name.into(), ty);
        self
    }

    /// Looks up a column type.
    pub fn column(&self, name: &str) -> Option<FormulaType> {
        self.columns.get(name).copied()
    }

    /// Looks up a relation column type.
    pub fn relation_column(&self, relation: &str, column: &str) -> Option<FormulaType> {
        self.relations
            .get(relation)
            .and_then(|columns| columns.get(column))
            .copied()
    }

    /// All known relation names and their columns.
    pub fn relations(&self) -> &BTreeMap<String, BTreeMap<String, FormulaType>> {
        &self.relations
    }
}

/// Parses and typechecks a formula in one step.
pub fn validate_expression(input: &str, env: &TypeEnv) -> Result<FormulaType, FormulaError> {
    let expression = parser::parse(input)?;
    typecheck(&expression, env)
}

/// Infers the result type of an expression.
pub fn typecheck(expression: &Expr, env: &TypeEnv) -> Result<FormulaType, FormulaError> {
    match expression {
        Expr::Literal(value) => match value {
            Value::Null => Ok(FormulaType::Any),
            Value::Text(_) | Value::Select(_) | Value::AttachmentRef(_) => Ok(FormulaType::Text),
            Value::Integer(_) => Ok(FormulaType::Integer),
            Value::Decimal(_) => Ok(FormulaType::Decimal),
            Value::Currency(_) => Ok(FormulaType::Currency),
            Value::Boolean(_) => Ok(FormulaType::Boolean),
            Value::Date(_) => Ok(FormulaType::Date),
            Value::Timestamp(_) => Ok(FormulaType::Timestamp),
        },
        Expr::Column { table: None, name } => env
            .column(name)
            .ok_or_else(|| FormulaError::UnknownColumn(name.clone())),
        Expr::Column { table: Some(_), .. } => Err(FormulaError::Type(
            "qualified columns are only allowed inside aggregate functions".into(),
        )),
        Expr::Unary { op, expr } => {
            let inner = typecheck(expr, env)?;
            match op {
                UnaryOp::Neg => {
                    if inner.is_numeric() || inner == FormulaType::Any {
                        Ok(inner)
                    } else {
                        Err(FormulaError::Type(format!(
                            "cannot negate {}",
                            inner.label()
                        )))
                    }
                }
                UnaryOp::Not => {
                    if matches!(inner, FormulaType::Boolean | FormulaType::Any) {
                        Ok(FormulaType::Boolean)
                    } else {
                        Err(FormulaError::Type(format!(
                            "NOT expects boolean, found {}",
                            inner.label()
                        )))
                    }
                }
            }
        }
        Expr::Binary { op, left, right } => {
            let left_type = typecheck(left, env)?;
            let right_type = typecheck(right, env)?;
            binary_type(*op, left_type, right_type)
        }
        Expr::Call { name, args } => call_type(name, args, env),
    }
}

fn binary_type(
    op: BinaryOp,
    left: FormulaType,
    right: FormulaType,
) -> Result<FormulaType, FormulaError> {
    match op {
        BinaryOp::And | BinaryOp::Or => {
            require_boolean(left, op.as_str())?;
            require_boolean(right, op.as_str())?;
            Ok(FormulaType::Boolean)
        }
        BinaryOp::Eq | BinaryOp::NotEq => {
            if comparable(left, right) {
                Ok(FormulaType::Boolean)
            } else {
                Err(FormulaError::Type(format!(
                    "cannot compare {} with {}",
                    left.label(),
                    right.label()
                )))
            }
        }
        BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte => {
            if ordered(left, right) {
                Ok(FormulaType::Boolean)
            } else {
                Err(FormulaError::Type(format!(
                    "{} cannot order {} and {}",
                    op.as_str(),
                    left.label(),
                    right.label()
                )))
            }
        }
        BinaryOp::Add | BinaryOp::Sub => {
            if let Some(result) = numeric_result(left, right) {
                return Ok(result);
            }
            if matches!(left, FormulaType::Currency | FormulaType::Any)
                && matches!(right, FormulaType::Currency | FormulaType::Any)
            {
                return Ok(FormulaType::Currency);
            }
            Err(FormulaError::Type(format!(
                "{} cannot add {} and {}",
                op.as_str(),
                left.label(),
                right.label()
            )))
        }
        BinaryOp::Mul => {
            if let Some(result) = numeric_result(left, right) {
                return Ok(result);
            }
            if matches!(left, FormulaType::Currency | FormulaType::Any)
                && (right.is_numeric() || right == FormulaType::Any)
            {
                return Ok(FormulaType::Currency);
            }
            if (left.is_numeric() || left == FormulaType::Any)
                && matches!(right, FormulaType::Currency | FormulaType::Any)
            {
                return Ok(FormulaType::Currency);
            }
            Err(FormulaError::Type(format!(
                "cannot multiply {} by {}",
                left.label(),
                right.label()
            )))
        }
        BinaryOp::Div => {
            if (left.is_numeric() || left == FormulaType::Any)
                && (right.is_numeric() || right == FormulaType::Any)
            {
                return Ok(if left == FormulaType::Any && right == FormulaType::Any {
                    FormulaType::Any
                } else {
                    FormulaType::Decimal
                });
            }
            if matches!(left, FormulaType::Currency | FormulaType::Any)
                && (right.is_numeric() || right == FormulaType::Any)
            {
                return Ok(FormulaType::Currency);
            }
            if matches!(left, FormulaType::Currency | FormulaType::Any)
                && matches!(right, FormulaType::Currency | FormulaType::Any)
            {
                return Ok(FormulaType::Decimal);
            }
            Err(FormulaError::Type(format!(
                "cannot divide {} by {}",
                left.label(),
                right.label()
            )))
        }
        BinaryOp::Rem => {
            if let Some(result) = numeric_result(left, right) {
                return Ok(result);
            }
            Err(FormulaError::Type(format!(
                "remainder expects numbers, found {} and {}",
                left.label(),
                right.label()
            )))
        }
    }
}

fn numeric_result(left: FormulaType, right: FormulaType) -> Option<FormulaType> {
    if !(left.is_numeric() || left == FormulaType::Any)
        || !(right.is_numeric() || right == FormulaType::Any)
    {
        return None;
    }
    if left == FormulaType::Integer && right == FormulaType::Integer {
        Some(FormulaType::Integer)
    } else if left == FormulaType::Any && right == FormulaType::Any {
        Some(FormulaType::Any)
    } else if left == FormulaType::Any {
        Some(right)
    } else if right == FormulaType::Any {
        Some(left)
    } else {
        Some(FormulaType::Decimal)
    }
}

fn require_boolean(ty: FormulaType, operator: &str) -> Result<(), FormulaError> {
    if matches!(ty, FormulaType::Boolean | FormulaType::Any) {
        Ok(())
    } else {
        Err(FormulaError::Type(format!(
            "{operator} expects boolean operands, found {}",
            ty.label()
        )))
    }
}

fn union_type(left: FormulaType, right: FormulaType) -> Option<FormulaType> {
    match (left, right) {
        (FormulaType::Any, other) | (other, FormulaType::Any) => Some(other),
        (left, right) if left == right => Some(left),
        (FormulaType::Integer, FormulaType::Decimal)
        | (FormulaType::Decimal, FormulaType::Integer) => Some(FormulaType::Decimal),
        _ => None,
    }
}

fn comparable(left: FormulaType, right: FormulaType) -> bool {
    if matches!(left, FormulaType::Any) || matches!(right, FormulaType::Any) {
        return true;
    }
    if left.is_numeric() && right.is_numeric() {
        return true;
    }
    left == right
}

fn ordered(left: FormulaType, right: FormulaType) -> bool {
    if matches!(left, FormulaType::Any) || matches!(right, FormulaType::Any) {
        return true;
    }
    if left.is_numeric() && right.is_numeric() {
        return true;
    }
    left == right
        && (left.is_numeric()
            || left.is_temporal()
            || matches!(left, FormulaType::Text | FormulaType::Currency))
}

fn call_type(name: &str, args: &[Expr], env: &TypeEnv) -> Result<FormulaType, FormulaError> {
    match name {
        "IF" => {
            expect_arity(name, args, 3)?;
            let condition = typecheck(&args[0], env)?;
            require_boolean(condition, "IF")?;
            let left = typecheck(&args[1], env)?;
            let right = typecheck(&args[2], env)?;
            union_type(left, right).ok_or_else(|| {
                FormulaError::Type(format!(
                    "IF branches have incompatible types {} and {}",
                    left.label(),
                    right.label()
                ))
            })
        }
        "COALESCE" => {
            if args.is_empty() {
                return Err(FormulaError::Arity {
                    function: name.into(),
                    expected: "at least one argument".into(),
                    found: 0,
                });
            }
            let mut result = FormulaType::Any;
            for arg in args {
                let ty = typecheck(arg, env)?;
                result = union_type(result, ty).ok_or_else(|| {
                    FormulaError::Type(format!(
                        "COALESCE arguments mix {} and {}",
                        result.label(),
                        ty.label()
                    ))
                })?;
            }
            Ok(result)
        }
        "CONCAT" => {
            if args.is_empty() {
                return Err(FormulaError::Arity {
                    function: name.into(),
                    expected: "at least one argument".into(),
                    found: 0,
                });
            }
            for arg in args {
                typecheck(arg, env)?;
            }
            Ok(FormulaType::Text)
        }
        "LEN" | "UPPER" | "LOWER" | "TRIM" => {
            expect_arity(name, args, 1)?;
            let ty = typecheck(&args[0], env)?;
            if !matches!(ty, FormulaType::Text | FormulaType::Any) {
                return Err(FormulaError::Type(format!(
                    "{name} expects text, found {}",
                    ty.label()
                )));
            }
            Ok(if name == "LEN" {
                FormulaType::Integer
            } else {
                FormulaType::Text
            })
        }
        "ABS" => {
            expect_arity(name, args, 1)?;
            let ty = typecheck(&args[0], env)?;
            if ty.is_numeric() || matches!(ty, FormulaType::Currency | FormulaType::Any) {
                Ok(ty)
            } else {
                Err(FormulaError::Type(format!(
                    "ABS expects a number, found {}",
                    ty.label()
                )))
            }
        }
        "ROUND" => {
            expect_arity(name, args, 2)?;
            let value = typecheck(&args[0], env)?;
            if !(value.is_numeric() || matches!(value, FormulaType::Currency | FormulaType::Any)) {
                return Err(FormulaError::Type(format!(
                    "ROUND expects a number, found {}",
                    value.label()
                )));
            }
            let digits = typecheck(&args[1], env)?;
            if !matches!(digits, FormulaType::Integer | FormulaType::Any) {
                return Err(FormulaError::Type(
                    "ROUND expects an integer digit count".into(),
                ));
            }
            Ok(value)
        }
        "DATE" => {
            expect_arity(name, args, 1)?;
            let ty = typecheck(&args[0], env)?;
            if !matches!(ty, FormulaType::Text | FormulaType::Date | FormulaType::Any) {
                return Err(FormulaError::Type(format!(
                    "DATE expects text, found {}",
                    ty.label()
                )));
            }
            Ok(FormulaType::Date)
        }
        "TIMESTAMP" => {
            expect_arity(name, args, 1)?;
            let ty = typecheck(&args[0], env)?;
            if !matches!(
                ty,
                FormulaType::Text | FormulaType::Timestamp | FormulaType::Any
            ) {
                return Err(FormulaError::Type(format!(
                    "TIMESTAMP expects text, found {}",
                    ty.label()
                )));
            }
            Ok(FormulaType::Timestamp)
        }
        "TODAY" => {
            expect_arity(name, args, 0)?;
            Ok(FormulaType::Date)
        }
        "NOW" => {
            expect_arity(name, args, 0)?;
            Ok(FormulaType::Timestamp)
        }
        "YEAR" | "MONTH" | "DAY" => {
            expect_arity(name, args, 1)?;
            let ty = typecheck(&args[0], env)?;
            if !(ty.is_temporal() || ty == FormulaType::Any) {
                return Err(FormulaError::Type(format!(
                    "{name} expects a date or timestamp, found {}",
                    ty.label()
                )));
            }
            Ok(FormulaType::Integer)
        }
        "COUNT" | "SUM" | "AVG" | "MIN" | "MAX" => {
            expect_arity(name, args, 1)?;
            let Some((relation, column)) = args[0].qualified_column() else {
                return Err(FormulaError::Type(format!(
                    "{name} expects a qualified child column such as [Line Items].[Amount]"
                )));
            };
            let Some(column_type) = env.relation_column(relation, column) else {
                return Err(FormulaError::UnknownRelation {
                    relation: relation.to_owned(),
                    column: column.to_owned(),
                });
            };
            let aggregate = aggregate_for(name);
            aggregate_type(aggregate, column_type)
        }
        other => Err(FormulaError::UnknownFunction(other.to_owned())),
    }
}

fn aggregate_for(name: &str) -> Aggregate {
    match name {
        "COUNT" => Aggregate::Count,
        "AVG" => Aggregate::Avg,
        "MIN" => Aggregate::Min,
        "MAX" => Aggregate::Max,
        _ => Aggregate::Sum,
    }
}

fn aggregate_type(
    aggregate: Aggregate,
    column_type: FormulaType,
) -> Result<FormulaType, FormulaError> {
    if aggregate == Aggregate::Count {
        return Ok(FormulaType::Integer);
    }
    if column_type == FormulaType::Any {
        return Ok(FormulaType::Any);
    }
    let numeric = column_type.is_numeric() || column_type == FormulaType::Currency;
    if !numeric {
        if matches!(aggregate, Aggregate::Min | Aggregate::Max)
            && (column_type == FormulaType::Text || column_type.is_temporal())
        {
            return Ok(column_type);
        }
        return Err(FormulaError::Type(format!(
            "{} expects a numeric child column, found {}",
            aggregate.as_str(),
            column_type.label()
        )));
    }
    Ok(match aggregate {
        Aggregate::Avg if column_type == FormulaType::Currency => FormulaType::Currency,
        Aggregate::Avg => FormulaType::Decimal,
        Aggregate::Sum | Aggregate::Min | Aggregate::Max => column_type,
        Aggregate::Count => FormulaType::Integer,
    })
}

fn expect_arity(name: &str, args: &[Expr], expected: usize) -> Result<(), FormulaError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(FormulaError::Arity {
            function: name.to_owned(),
            expected: format!("{expected} argument(s)"),
            found: args.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slab_core::{Column, CurrencyCode, DeleteAction, Relation, Table};

    fn env() -> TypeEnv {
        TypeEnv::default()
            .with_column("Subtotal", FormulaType::Currency)
            .with_column("Tax", FormulaType::Currency)
            .with_column("Name", FormulaType::Text)
            .with_column("Qty", FormulaType::Integer)
            .with_column("Price", FormulaType::Decimal)
            .with_column("Due", FormulaType::Date)
            .with_column("Updated", FormulaType::Timestamp)
    }

    fn assert_types(env: &TypeEnv, cases: &[(&str, FormulaType)]) {
        for &(source, expected) in cases {
            assert_eq!(
                validate_expression(source, env).expect(source),
                expected,
                "{source}"
            );
        }
    }

    fn assert_type_errors(env: &TypeEnv, sources: &[&str]) {
        for &source in sources {
            let error = validate_expression(source, env).expect_err(source);
            assert!(matches!(error, FormulaError::Type(_)), "{source}: {error}");
            assert_eq!(error.code_str(), "SLAB_E_FORMULA_TYPE", "{source}: {error}");
        }
    }

    #[test]
    fn type_labels_and_categories() {
        use FormulaType::*;
        for (ty, label, numeric, temporal) in [
            (Any, "any", false, false),
            (Text, "text", false, false),
            (Integer, "integer", true, false),
            (Decimal, "decimal", true, false),
            (Currency, "currency", false, false),
            (Boolean, "boolean", false, false),
            (Date, "date", false, true),
            (Timestamp, "timestamp", false, true),
        ] {
            assert_eq!(ty.label(), label);
            assert_eq!(ty.is_numeric(), numeric, "{label}");
            assert_eq!(ty.is_temporal(), temporal, "{label}");
        }
    }

    #[test]
    fn arithmetic_promotes_numbers_and_preserves_currency() {
        use FormulaType::*;
        let env = env();
        assert_types(
            &env,
            &[
                ("-Qty", Integer),
                ("-NULL", Any),
                ("Qty + Qty", Integer),
                ("Qty - Price", Decimal),
                ("Qty % Price", Decimal),
                ("NULL + Qty", Integer),
                ("Qty + NULL", Integer),
                ("NULL + NULL", Any),
                ("Subtotal + Tax", Currency),
                ("Subtotal - NULL", Currency),
                ("Qty * Price", Decimal),
                ("Subtotal * Qty", Currency),
                ("Qty * Tax", Currency),
                ("Subtotal * NULL", Currency),
                ("NULL * Tax", Currency),
                ("Qty / Qty", Decimal),
                ("NULL / Qty", Decimal),
                ("Qty / NULL", Decimal),
                ("NULL / NULL", Any),
                ("Subtotal / Qty", Currency),
                ("Subtotal / NULL", Currency),
                ("Subtotal / Tax", Decimal),
                ("NULL / Tax", Decimal),
            ],
        );
        assert_type_errors(
            &env,
            &[
                "-Name",
                "Subtotal + Name",
                "Name + Qty",
                "Qty + Name",
                "Subtotal % Qty",
                "Subtotal * Tax",
                "Subtotal * Name",
                "Name * Tax",
                "Qty * Name",
                "Subtotal / Name",
                "Name / Tax",
                "Name / Qty",
                "Qty / Name",
                "Qty / Tax",
            ],
        );
    }

    #[test]
    fn comparisons_distinguish_equality_ordering_and_logical_operands() {
        use FormulaType::Boolean;
        let env = env();
        assert_types(
            &env,
            &[
                ("Name = Name", Boolean),
                ("Qty != Price", Boolean),
                ("TRUE = NULL", Boolean),
                ("Name < Name", Boolean),
                ("Due <= Due", Boolean),
                ("Updated > Updated", Boolean),
                ("Subtotal >= Tax", Boolean),
                ("Qty < Price", Boolean),
                ("NULL < TRUE", Boolean),
                ("NOT TRUE OR FALSE AND NULL", Boolean),
            ],
        );
        assert_type_errors(
            &env,
            &[
                "Name = Qty",
                "TRUE != Name",
                "Qty < Name",
                "Due < Updated",
                "TRUE < FALSE",
                "TRUE AND Qty",
                "Qty OR FALSE",
                "NOT Qty",
            ],
        );
    }

    #[test]
    fn builtins_infer_result_types_and_reject_invalid_arguments() {
        use FormulaType::*;
        let env = env();
        assert_types(
            &env,
            &[
                ("IF(Qty > 1, Name, Name)", Text),
                ("IF(TRUE, Qty, Price)", Decimal),
                ("IF(NULL, Name, NULL)", Text),
                ("COALESCE(NULL, Qty, Price)", Decimal),
                ("CONCAT(Name, Qty)", Text),
                ("len(Name)", Integer),
                ("UPPER(Name)", Text),
                ("LOWER(NULL)", Text),
                ("TRIM(Name)", Text),
                ("ABS(Qty)", Integer),
                ("ABS(Subtotal)", Currency),
                ("ABS(NULL)", Any),
                ("ROUND(Price, Qty)", Decimal),
                ("ROUND(Subtotal, NULL)", Currency),
                ("DATE('2024-02-29')", Date),
                ("DATE(Due)", Date),
                ("TODAY()", Date),
                ("TIMESTAMP('2024-02-29T00:00:00Z')", Timestamp),
                ("TIMESTAMP(Updated)", Timestamp),
                ("NOW()", Timestamp),
                ("YEAR(Due)", Integer),
                ("MONTH(Updated)", Integer),
                ("DAY(NULL)", Integer),
            ],
        );
        assert_type_errors(
            &env,
            &[
                "IF(Qty, 1, 2)",
                "IF(TRUE, Name, Subtotal)",
                "COALESCE(Name, Qty)",
                "LEN(Qty)",
                "ABS(Name)",
                "ROUND(Name, 1)",
                "ROUND(Price, 1.5)",
                "DATE(Qty)",
                "TIMESTAMP(Qty)",
                "YEAR(Qty)",
            ],
        );

        for (source, function, found) in [
            ("IF(TRUE, 1)", "IF", 2),
            ("LEN()", "LEN", 0),
            ("NOW(1)", "NOW", 1),
            ("COALESCE()", "COALESCE", 0),
            ("CONCAT()", "CONCAT", 0),
        ] {
            let error = validate_expression(source, &env).expect_err(source);
            assert!(
                matches!(error, FormulaError::Arity {function: ref name, found: count, ..}
                if name == function && count == found),
                "{source}: {error}"
            );
        }
    }

    fn table(name: &str, columns: Vec<(&str, ColumnType)>) -> Table {
        Table {
            id: name.into(),
            name: name.into(),
            columns: columns
                .into_iter()
                .enumerate()
                .map(|(position, (column, ty))| Column {
                    id: format!("{name}.{column}"),
                    table: name.into(),
                    name: column.into(),
                    ty,
                    required: false,
                    unique: false,
                    default: None,
                    position: u32::try_from(position).expect("test column index fits"),
                })
                .collect(),
            created_at: 0,
        }
    }

    #[test]
    fn infers_aggregates_from_child_columns_in_the_schema() {
        use FormulaType::*;
        let money = ColumnType::Currency {
            currency: CurrencyCode::parse("USD").expect("currency"),
        };
        let schema = Schema {
            tables: vec![
                table("Invoices", vec![("Total", money.clone())]),
                table(
                    "Lines",
                    vec![
                        ("Invoice", ColumnType::Integer),
                        ("Qty", ColumnType::Integer),
                        ("Amount", money),
                        ("Description", ColumnType::Text),
                        ("Due", ColumnType::Date),
                        ("Taxable", ColumnType::Boolean),
                        (
                            "Estimate",
                            ColumnType::Formula {
                                expression: "Amount".into(),
                            },
                        ),
                    ],
                ),
            ],
            relations: vec![Relation {
                id: "invoice-lines".into(),
                name: "Line Items".into(),
                parent_table: "Invoices".into(),
                child_table: "Lines".into(),
                child_column: "Invoice".into(),
                on_delete: DeleteAction::Cascade,
            }],
            layouts: Vec::new(),
        };
        schema.validate().expect("valid schema");
        let env = TypeEnv::for_table(&schema, "Invoices").expect("invoice environment");
        assert_eq!(
            env.relations()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["Line Items"]
        );
        assert_eq!(env.relation_column("Line Items", "id"), Some(Integer));
        assert_eq!(env.column("Description"), None);
        assert_types(
            &env,
            &[
                ("id", Integer),
                ("Total", Currency),
                ("COUNT([Line Items].[Taxable])", Integer),
                ("COUNT([Line Items].[Estimate])", Integer),
                ("SUM([Line Items].[Qty])", Integer),
                ("SUM([Line Items].[Amount])", Currency),
                ("SUM([Line Items].[Estimate])", Any),
                ("AVG([Line Items].[Qty])", Decimal),
                ("AVG([Line Items].[Amount])", Currency),
                ("MIN([Line Items].[Description])", Text),
                ("MAX([Line Items].[Due])", Date),
            ],
        );
        assert_type_errors(
            &env,
            &[
                "SUM([Line Items].[Description])",
                "AVG([Line Items].[Due])",
                "MAX([Line Items].[Taxable])",
                "SUM(Total)",
                "[Line Items].[Amount]",
            ],
        );

        for (relation, column) in [("Unknown Lines", "Amount"), ("Line Items", "Missing")] {
            let error = validate_expression(&format!("SUM([{relation}].[{column}])"), &env)
                .expect_err("unresolved child reference");
            assert!(
                matches!(error, FormulaError::UnknownRelation { relation: ref name, column: ref key }
                if name == relation && key == column),
                "{error}"
            );
        }
    }

    #[test]
    fn unknown_column_is_reported() {
        let error = validate_expression("[Missing] + 1", &env()).expect_err("reject");
        assert_eq!(error.code_str(), "SLAB_E_FORMULA_REF");
    }
}
