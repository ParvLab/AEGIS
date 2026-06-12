use crate::error::AegisResult;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ConditionEvalContext {
    pub subject_meta: HashMap<String, String>,
    pub resource_meta: HashMap<String, String>,
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub enum ConditionOp {
    Eq(String),
    Neq(String),
    In(Vec<String>),
    Exists,
    Gt(String),
    Lt(String),
}

#[derive(Debug, Clone)]
pub struct ConditionExpr {
    pub attr: String,
    pub op: ConditionOp,
}

pub fn parse_condition(expr: &str) -> AegisResult<ConditionExpr> {
    let parts: Vec<&str> = expr.splitn(2, char::is_whitespace).collect();
    if parts.len() < 2 {
        return Err(crate::error::AegisError::SchemaValidation(
            format!("invalid condition expression: {:?}", expr),
        ));
    }
    let attr = parts[0].to_string();
    let rest = parts[1].trim().to_string();

    if let Some(val) = rest.strip_prefix("eq ") {
        Ok(ConditionExpr {
            attr,
            op: ConditionOp::Eq(val.trim().to_string()),
        })
    } else if let Some(val) = rest.strip_prefix("neq ") {
        Ok(ConditionExpr {
            attr,
            op: ConditionOp::Neq(val.trim().to_string()),
        })
    } else if let Some(val) = rest.strip_prefix("in ") {
        let items: Vec<String> = val
            .trim()
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        Ok(ConditionExpr {
            attr,
            op: ConditionOp::In(items),
        })
    } else if rest == "exists" {
        Ok(ConditionExpr {
            attr,
            op: ConditionOp::Exists,
        })
    } else if let Some(val) = rest.strip_prefix("gt ") {
        Ok(ConditionExpr {
            attr,
            op: ConditionOp::Gt(val.trim().to_string()),
        })
    } else if let Some(val) = rest.strip_prefix("lt ") {
        Ok(ConditionExpr {
            attr,
            op: ConditionOp::Lt(val.trim().to_string()),
        })
    } else {
        Err(crate::error::AegisError::SchemaValidation(
            format!("unknown condition operator in: {:?}", expr),
        ))
    }
}

pub fn evaluate_condition(expr: &ConditionExpr, ctx: &ConditionEvalContext) -> bool {
    let value = ctx
        .subject_meta
        .get(&expr.attr)
        .or_else(|| ctx.resource_meta.get(&expr.attr))
        .or_else(|| ctx.env.get(&expr.attr));

    match &expr.op {
        ConditionOp::Eq(expected) => value.map_or(false, |v| v == expected),
        ConditionOp::Neq(expected) => value.map_or(true, |v| v != expected),
        ConditionOp::In(items) => value.map_or(false, |v| items.contains(v)),
        ConditionOp::Exists => value.is_some(),
        ConditionOp::Gt(expected) => value
            .and_then(|v| v.parse::<f64>().ok())
            .zip(expected.parse::<f64>().ok())
            .map_or(false, |(v, e)| v > e),
        ConditionOp::Lt(expected) => value
            .and_then(|v| v.parse::<f64>().ok())
            .zip(expected.parse::<f64>().ok())
            .map_or(false, |(v, e)| v < e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ConditionEvalContext {
        let mut subject_meta = HashMap::new();
        subject_meta.insert("role".to_string(), "admin".to_string());
        subject_meta.insert("score".to_string(), "95".to_string());
        let mut resource_meta = HashMap::new();
        resource_meta.insert("region".to_string(), "us-east".to_string());
        ConditionEvalContext {
            subject_meta,
            resource_meta,
            env: HashMap::new(),
        }
    }

    #[test]
    fn test_eq_matches() {
        let expr = parse_condition("role eq admin").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_eq_no_match() {
        let expr = parse_condition("role eq viewer").unwrap();
        assert!(!evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_neq_no_match() {
        let expr = parse_condition("role neq viewer").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_exists() {
        let expr = parse_condition("role exists").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_not_exists() {
        let expr = parse_condition("missing exists").unwrap();
        assert!(!evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_in() {
        let expr = parse_condition("role in [admin, moderator]").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_in_not_found() {
        let expr = parse_condition("role in [viewer, editor]").unwrap();
        assert!(!evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_gt() {
        let expr = parse_condition("score gt 90").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_lt() {
        let expr = parse_condition("score lt 100").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_resource_meta() {
        let expr = parse_condition("region eq us-east").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_invalid_syntax() {
        assert!(parse_condition("bad input here").is_err());
    }

    #[test]
    fn test_empty_expr() {
        assert!(parse_condition("").is_err());
    }
}
