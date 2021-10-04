use {
    crate::{
        ast::{ColumnDef, Expr},
        data::{schema::ColumnDefExt, Value},
        executor::evaluate_stateless,
        result::Result,
    },
    serde::{Deserialize, Serialize},
    std::{collections::HashMap, fmt::Debug},
    thiserror::Error,
};

#[derive(Error, Serialize, Debug, PartialEq)]
pub enum RowError {
    #[error("lack of required column: {0}")]
    LackOfRequiredColumn(String),

    #[error("column and values not matched")]
    ColumnAndValuesNotMatched,

    #[error("literals have more values than target columns")]
    TooManyValues,

    #[error("conflict! row cannot be empty")]
    ConflictOnEmptyRow,
}

#[derive(iter_enum::Iterator)]
enum Columns<I1, I2> {
    All(I1),
    Specified(I2),
}

/*
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Row(Vec<Value>);
*/

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Row {
    Vec(Vec<Value>),
    Map(HashMap<String, Value>),
}

impl From<Vec<Value>> for Row {
    fn from(values: Vec<Value>) -> Self {
        Self::Vec(values)
    }
}

impl Row {
    pub fn get_value(&self, index: usize) -> Option<&Value> {
        match self {
            Self::Vec(values) => values.get(index),
            Self::Map(_) => None,
        }
    }

    pub fn into_values(self) -> Vec<Value> {
        match self {
            Self::Vec(values) => values,
            Self::Map(_) => vec![],
        }
    }

    pub fn take_first_value(self) -> Result<Value> {
        let values = match self {
            Self::Vec(values) => values,
            Self::Map(_) => vec![],
        };

        values
            .into_iter()
            .next()
            .ok_or_else(|| RowError::ConflictOnEmptyRow.into())
    }

    pub fn new(column_defs: &[ColumnDef], columns: &[String], values: &[Expr]) -> Result<Self> {
        if !columns.is_empty() && values.len() != columns.len() {
            return Err(RowError::ColumnAndValuesNotMatched.into());
        } else if values.len() > column_defs.len() {
            return Err(RowError::TooManyValues.into());
        }

        let columns = if columns.is_empty() {
            Columns::All(column_defs.iter().map(|ColumnDef { name, .. }| name))
        } else {
            Columns::Specified(columns.iter())
        };

        let column_name_value_list = columns.zip(values.iter()).collect::<Vec<(_, _)>>();

        column_defs
            .iter()
            .map(|column_def| {
                let ColumnDef {
                    name: def_name,
                    data_type,
                    ..
                } = column_def;

                let value = column_name_value_list
                    .iter()
                    .find(|(name, _)| name == &def_name)
                    .map(|(_, value)| value);

                let nullable = column_def.is_nullable();

                match (value, column_def.get_default(), nullable) {
                    (Some(&expr), _, _) | (None, Some(expr), _) => {
                        evaluate_stateless(None, expr)?.try_into_value(data_type, nullable)
                    }
                    (None, None, true) => Ok(Value::Null),
                    (None, None, false) => {
                        Err(RowError::LackOfRequiredColumn(def_name.to_owned()).into())
                    }
                }
            })
            .collect::<Result<_>>()
            .map(Self::Vec)
    }

    pub fn validate(&self, column_defs: &[ColumnDef]) -> Result<()> {
        let items = column_defs
            .iter()
            .enumerate()
            .filter_map(|(index, column_def)| {
                let value = self.get_value(index);

                value.map(|v| (v, column_def))
            });

        for (value, column_def) in items {
            let ColumnDef { data_type, .. } = column_def;
            let nullable = column_def.is_nullable();

            value.validate_type(data_type)?;
            value.validate_null(nullable)?;
        }

        Ok(())
    }
}
