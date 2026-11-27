//! Formula dependency graph and deterministic cycle rejection.

use std::collections::{BTreeMap, BTreeSet};

use slab_core::{ColumnType, Schema};

use crate::error::FormulaError;
use crate::parser;
use crate::types::{self, FormulaType, TypeEnv};

type Node = (String, String);

/// The dependency graph for every formula column in a schema.
#[derive(Clone, Debug, Default)]
pub struct FormulaDag {
    dependencies: BTreeMap<Node, BTreeSet<Node>>,
}

/// The outcome of checking a proposed formula.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaCheck {
    /// The inferred result type.
    pub result: FormulaType,
    /// Same-table formula columns this expression depends on.
    pub dependencies: Vec<String>,
}

impl FormulaDag {
    /// Builds the graph from a schema and validates every table.
    pub fn build(schema: &Schema) -> Result<Self, FormulaError> {
        let mut dag = Self::default();
        for table in &schema.tables {
            for column in &table.columns {
                if let ColumnType::Formula { expression } = &column.ty {
                    let analyzed = analyze(schema, &table.name, &column.name, expression)?;
                    dag.dependencies.insert(
                        (table.name.clone(), column.name.clone()),
                        analyzed
                            .into_iter()
                            .map(|dependency| (table.name.clone(), dependency))
                            .collect(),
                    );
                }
            }
        }
        for table in &schema.tables {
            dag.evaluation_order(&table.name)?;
        }
        Ok(dag)
    }

    /// Validates a schema without keeping the graph.
    pub fn validate_schema(schema: &Schema) -> Result<(), FormulaError> {
        Self::build(schema).map(|_| ())
    }

    /// Checks a proposed formula, including cycle rejection, without mutating
    /// the graph.
    pub fn check(
        &self,
        schema: &Schema,
        table: &str,
        column: &str,
        expression: &str,
    ) -> Result<FormulaCheck, FormulaError> {
        let env = TypeEnv::for_table(schema, table)?.with_column(column, FormulaType::Any);
        let parsed = parser::parse(expression)?;
        let result = types::typecheck(&parsed, &env)?;
        let dependencies = analyze(schema, table, column, expression)?;
        let mut candidate = self.clone();
        candidate.dependencies.insert(
            (table.to_owned(), column.to_owned()),
            dependencies
                .iter()
                .map(|dependency| (table.to_owned(), dependency.clone()))
                .collect(),
        );
        candidate.evaluation_order(table)?;
        Ok(FormulaCheck {
            result,
            dependencies,
        })
    }

    /// Inserts or replaces a formula after checking it. On error the graph is
    /// unchanged.
    pub fn insert(
        &mut self,
        schema: &Schema,
        table: &str,
        column: &str,
        expression: &str,
    ) -> Result<FormulaCheck, FormulaError> {
        let check = self.check(schema, table, column, expression)?;
        self.dependencies.insert(
            (table.to_owned(), column.to_owned()),
            check
                .dependencies
                .iter()
                .map(|dependency| (table.to_owned(), dependency.clone()))
                .collect(),
        );
        Ok(check)
    }

    /// Removes a formula node and any edges that referenced it.
    pub fn remove_column(&mut self, table: &str, column: &str) {
        let node = (table.to_owned(), column.to_owned());
        self.dependencies.remove(&node);
        for dependencies in self.dependencies.values_mut() {
            dependencies.remove(&node);
        }
    }

    /// Formula columns in one table.
    pub fn formula_columns(&self, table: &str) -> impl Iterator<Item = &str> {
        self.dependencies
            .keys()
            .filter(move |(owner, _)| owner == table)
            .map(|(_, column)| column.as_str())
    }

    /// Same-table formula dependencies of one column.
    pub fn dependencies(&self, table: &str, column: &str) -> impl Iterator<Item = &str> {
        self.dependencies
            .get(&(table.to_owned(), column.to_owned()))
            .into_iter()
            .flatten()
            .map(|(_, name)| name.as_str())
    }

    /// Evaluation order for the formula columns of one table.
    pub fn evaluation_order(&self, table: &str) -> Result<Vec<String>, FormulaError> {
        let mut nodes: BTreeSet<String> = self
            .dependencies
            .keys()
            .filter(|(owner, _)| owner == table)
            .map(|(_, column)| column.clone())
            .collect();
        if nodes.is_empty() {
            return Ok(Vec::new());
        }
        let mut indegree: BTreeMap<String, usize> = BTreeMap::new();
        let mut dependents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for node in &nodes {
            let dependencies = self
                .dependencies
                .get(&(table.to_owned(), node.clone()))
                .into_iter()
                .flatten()
                .filter(|(_, dependency)| nodes.contains(dependency))
                .map(|(_, dependency)| dependency.clone());
            let mut count = 0;
            for dependency in dependencies {
                count += 1;
                dependents
                    .entry(dependency)
                    .or_default()
                    .insert(node.clone());
            }
            indegree.insert(node.clone(), count);
        }
        let mut ready: BTreeSet<String> = indegree
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(node, _)| node.clone())
            .collect();
        let mut order = Vec::with_capacity(nodes.len());
        while let Some(node) = ready.pop_first() {
            order.push(node.clone());
            nodes.remove(&node);
            if let Some(children) = dependents.get(&node) {
                for child in children {
                    if let Some(count) = indegree.get_mut(child) {
                        *count = count.saturating_sub(1);
                        if *count == 0 {
                            ready.insert(child.clone());
                        }
                    }
                }
            }
        }
        if !nodes.is_empty() {
            return Err(FormulaError::Cycle {
                path: cycle_path(table, &nodes, &self.dependencies),
            });
        }
        Ok(order)
    }
}

fn cycle_path(
    table: &str,
    remaining: &BTreeSet<String>,
    dependencies: &BTreeMap<Node, BTreeSet<Node>>,
) -> String {
    let Some(start) = remaining.iter().next() else {
        return String::new();
    };
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut path: Vec<String> = Vec::new();
    let mut current = start.clone();
    loop {
        if let Some(index) = seen.get(&current) {
            path.push(current);
            return path[*index..].join(" -> ");
        }
        seen.insert(current.clone(), path.len());
        path.push(current.clone());
        let next = dependencies
            .get(&(table.to_owned(), current.clone()))
            .into_iter()
            .flatten()
            .filter(|(_, dependency)| remaining.contains(dependency))
            .map(|(_, dependency)| dependency.clone())
            .min();
        match next {
            Some(next) => current = next,
            None => {
                path.push(current);
                return path.join(" -> ");
            }
        }
    }
}

fn analyze(
    schema: &Schema,
    table: &str,
    column: &str,
    expression: &str,
) -> Result<Vec<String>, FormulaError> {
    let table_definition = schema
        .table(table)
        .ok_or_else(|| FormulaError::UnknownTable(table.to_owned()))?;
    let env = TypeEnv::for_table(schema, table)?.with_column(column, FormulaType::Any);
    let parsed = parser::parse(expression)?;
    types::typecheck(&parsed, &env)?;
    let mut dependencies = BTreeSet::new();
    for reference in parsed.bare_columns() {
        let Some(target) = table_definition.column(&reference) else {
            return Err(FormulaError::UnknownColumn(reference));
        };
        if target.ty.is_computed() {
            dependencies.insert(reference);
        }
    }
    Ok(dependencies.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use slab_core::{Column, Table};

    fn formula_column(name: &str, expression: &str) -> Column {
        Column {
            id: format!("c_{name}"),
            table: "Invoices".into(),
            name: name.into(),
            ty: ColumnType::Formula {
                expression: expression.into(),
            },
            required: false,
            unique: false,
            default: None,
            position: 0,
        }
    }

    fn schema(columns: Vec<Column>) -> Schema {
        Schema {
            tables: vec![Table {
                id: "t_invoices".into(),
                name: "Invoices".into(),
                columns,
                created_at: 0,
            }],
            relations: Vec::new(),
            layouts: Vec::new(),
        }
    }

    #[test]
    fn orders_dependencies_before_dependents() {
        let mut quantity = formula_column("Qty", "0");
        quantity.ty = ColumnType::Integer;
        let mut invoices = schema(vec![
            formula_column("A_Total", "[Subtotal] + [Tax]"),
            formula_column("Subtotal", "[Qty] * 10"),
            formula_column("Tax", "[Subtotal] * 0.1"),
            quantity,
        ]);
        let mut shipping = formula_column("Shipping", "5");
        shipping.table = "Orders".into();
        invoices.tables.push(Table {
            id: "t_orders".into(),
            name: "Orders".into(),
            columns: vec![shipping],
            created_at: 0,
        });
        let mut dag = FormulaDag::build(&invoices).expect("formula graph");
        assert_eq!(
            dag.evaluation_order("Invoices").expect("order"),
            ["Subtotal", "Tax", "A_Total"]
        );
        assert_eq!(
            dag.formula_columns("Invoices").collect::<Vec<_>>(),
            ["A_Total", "Subtotal", "Tax"]
        );
        assert_eq!(
            dag.dependencies("Invoices", "A_Total").collect::<Vec<_>>(),
            ["Subtotal", "Tax"]
        );
        assert_eq!(dag.dependencies("Invoices", "Subtotal").count(), 0);

        dag.remove_column("Invoices", "Tax");
        assert_eq!(
            dag.evaluation_order("Invoices").expect("order"),
            ["Subtotal", "A_Total"]
        );
        assert_eq!(
            dag.dependencies("Invoices", "A_Total").collect::<Vec<_>>(),
            ["Subtotal"]
        );
    }

    #[test]
    fn rejects_cycles_deterministically() {
        let mut invoicing = schema(Vec::new());
        invoicing.tables[0]
            .columns
            .push(formula_column("A", "[B] + 1"));
        invoicing.tables[0]
            .columns
            .push(formula_column("B", "[C] + 1"));
        invoicing.tables[0]
            .columns
            .push(formula_column("C", "[A] + 1"));
        let first = FormulaDag::build(&invoicing).expect_err("cycle");
        let second = FormulaDag::build(&invoicing).expect_err("cycle");
        assert_eq!(first.to_string(), second.to_string());
        assert!(matches!(first, FormulaError::Cycle { path } if path == "A -> B -> C -> A"));
        assert!(FormulaDag::validate_schema(&invoicing).is_err());
    }

    #[test]
    fn self_reference_is_a_cycle() {
        let schema = schema(vec![formula_column("Total", "[Total] + 1")]);
        let error = FormulaDag::build(&schema).expect_err("cycle");
        assert!(matches!(error, FormulaError::Cycle { .. }));
    }
}
