mod parser;
mod types;
mod validator;

pub use parser::parse_schema;
pub use types::*;
pub use validator::{
    check_schema_compatibility,
    lint_schema,
    LintReport,
    validate_relation,
    validate_resource_type,
};
