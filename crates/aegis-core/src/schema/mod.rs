mod parser;
mod types;
mod validator;

pub use parser::parse_schema;
pub use types::*;
pub use validator::{
    LintReport, check_schema_compatibility, lint_schema, validate_relation, validate_resource_type,
};
