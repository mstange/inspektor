use futures::StreamExt;
use md5::digest::generic_array::typenum::Len;
use sqlparser::ast::{Expr, SelectItem, ObjectName};
use sqlparser::ast::{Ident, Value};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

// validation state contains all the required metadata that will be used for
// validating the selections.
#[derive(Debug, Default, Clone)]
pub struct Ctx {
    // global_allowed_selections holds all the allowed column name for the entire query statement.
    // eg: cte and processed sub query.
    global_allowed_selections: HashMap<String, Vec<String>>,
    // allowed_selection hold the selections that is allowed only for the current execution block.
    allowed_selections: BTreeMap<String, Vec<String>>,
    //table info holds the informaion about the table.
    table_info: HashMap<String, Vec<String>>,

    protected_columns: HashMap<String, Vec<String>>,

    from: HashSet<String>,
}

impl Ctx {
    pub fn new(table_info: HashMap<String, Vec<String>>) -> Ctx {
        let mut state = Ctx::default();
        state.table_info = table_info;
        state
    }
    pub fn insert_allowed_columns(&mut self, table_name: String, mut columns: Vec<String>) {
        self.allowed_selections.insert(table_name, columns);
    }

    pub fn get_columns(&self, table_name: &String) -> Option<Vec<String>> {
        if let Some(columns) = self.table_info.get(table_name) {
            return Some(columns.clone());
        }
        None
    }

    pub fn get_default_table(&self) -> Option<String> {
        if self.allowed_selections.len() == 1 {
            for (key, _) in &self.allowed_selections {
                return Some(key.to_string());
            }
        }
        None
    }

    pub fn is_allowed_column(&self, table_name: &String, column: &String) -> bool {
        if let Some(columns) = self.protected_columns.get(table_name) {
            return columns
                .iter()
                .position(|protectec_columns| *protectec_columns == *column)
                .is_none();
        }
        return true;
    }

    pub fn is_allowed_column_ident(&self, column: &String) -> bool {
        let froms = self.from.clone().into_iter().collect::<Vec<String>>();
        for from in &froms{
            if let Some(protected_columns) = self.protected_columns.get(from) {
                match protected_columns.iter().position(|col| *col==*column) {
                    Some(_) => return false,
                    None => continue
                }
            }
        }
        return true;
    }

    // is_valid_column check whether the given column is in the table info.
    pub fn is_valid_column(&self, table_name: Option<&String>, column: &String) -> bool {
        // if the table name given then check the column in valid for the given table name.
        if let Some(table_name) = table_name {
            let columns = match self.table_info.get(table_name) {
                Some(columns) => columns,
                _ => return false,
            };
            return columns
                .iter()
                .position(|table_column| -> bool { *table_column == *column })
                .is_some();
        }
        // otherwise just iterate over all the table info.
        for (_, columns) in &self.table_info {
            match columns
                .iter()
                .position(|table_column| *table_column == *column)
            {
                Some(_) => return true,
                None => continue,
            };
        }
        return false;
    }

    pub fn get_allowed_columns(&self, table_name: &String) -> Option<Vec<String>> {
        if let Some(columns) = self.allowed_selections.get(table_name) {
            return Some(
                columns
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<String>>(),
            );
        }
        None
    }

    pub fn merge_table_info(&mut self, table_name: String, state: Ctx) {
        for (_, val) in state.allowed_selections {
            self.table_info.insert(table_name.clone(), val);
        }
    }

    pub fn merge_allowed_selections(&mut self, table_name: String, state: Ctx) {
        for (_, val) in state.allowed_selections {
            self.allowed_selections.insert(table_name.clone(), val);
        }
    }

    pub fn merge_state(&mut self, state: Ctx) {
        for (key, val) in state.protected_columns {
            self.protected_columns.insert(key, val);
        }
        for (key, val) in state.table_info {
            self.table_info.insert(key, val);
        }
        for val in state.from {
            self.from.insert(val);
        }
    }

    pub fn build_allowed_column_expr(&self) -> Vec<SelectItem> {
        let mut selections = vec![];
        let mut wildcard = true;
        let mut froms = self.from.clone().into_iter().collect::<Vec<String>>();
        froms.sort();
        for from in froms{
            let exprs = self.column_expr_for_table(&from);
            if exprs.len() == 0 {
                selections.push(SelectItem::QualifiedWildcard(ObjectName(vec![Ident::new(from)])));
                continue;
            }
            wildcard = false;
            selections.extend_from_slice(&exprs[..]);    
        }
        if wildcard {
            return vec![SelectItem::Wildcard]
        }
        return selections;
    }

    pub fn column_expr_for_table(&self, table_name: &String) -> Vec<SelectItem> {
        let mut selections = vec![];
        if let Some(protected_columns) = self.protected_columns.get(table_name) {
            let protected_columns_set = protected_columns.iter().collect::<HashSet<&String>>();
            let table_columns = self.table_info.get(table_name).unwrap();
            for col in table_columns {
                if protected_columns_set.contains(col) {
                    selections.push(SelectItem::ExprWithAlias {
                        expr: Expr::Value(Value::Null),
                        alias:Ident { value: format!("{}.{}", table_name, col), quote_style: Some('"')},
                    });
                    continue;
                }
                selections.push(SelectItem::UnnamedExpr(Expr::CompoundIdentifier(vec![
                    Ident::new(table_name.to_string()),
                    Ident::new(col.to_string()),
                ])));
            }
        }
        return selections;
    }

    pub fn memorize_protected_columns(
        &mut self,
        table_name: String,
        protected_columns: Vec<String>,
    ) {
        self.protected_columns.insert(table_name, protected_columns);
    }

    pub fn get_protected_columns(&self, table_name: &String) -> Option<Vec<String>> {
        if let Some(protected_columns) = self.protected_columns.get(table_name) {
            return Some(
                protected_columns
                    .iter()
                    .map(|c| c.clone())
                    .collect::<Vec<String>>(),
            );
        }
        return None;
    }

    pub fn merge_protected_columns(&mut self, alias_name: String, state: Ctx) {
        for (_, val) in state.protected_columns {
            self.protected_columns.insert(alias_name.clone(), val);
        }
    }

    pub fn overwrite_table_info(&mut self, table_name: &String, alias: String) {
        if let Some(columns) = self.table_info.get(table_name) {
            self.table_info.insert(alias, columns.clone());
            return;
        }
        self.table_info.insert(alias, vec![]);
    }

    pub fn add_from_src(&mut self, table_name: String) {
        self.from.insert(table_name);
    }
}
