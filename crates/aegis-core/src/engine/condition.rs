use crate::error::AegisResult;
use chrono::{Datelike, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
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
    NotExists,
    Gt(String),
    Lt(String),
    Before(String),
    After(String),
    DayOfWeek(Vec<String>),
}

#[derive(Debug, Clone)]
pub enum ConditionExpr {
    Leaf { attr: String, op: ConditionOp },
    And(Vec<ConditionExpr>),
    Or(Vec<ConditionExpr>),
    Not(Box<ConditionExpr>),
}

pub fn parse_condition(expr: &str) -> AegisResult<ConditionExpr> {
    let trimmed = expr.trim();

    // Composite: NOT (expr)
    if let Some(inner) = trimmed.strip_prefix("NOT ") {
        let inner = inner.trim();
        if inner.starts_with('(') && inner.ends_with(')') {
            let inner_expr = parse_condition(&inner[1..inner.len() - 1])?;
            return Ok(ConditionExpr::Not(Box::new(inner_expr)));
        }
        return Err(crate::error::AegisError::SchemaValidation(format!(
            "NOT condition must be parenthesized: {:?}",
            expr
        )));
    }

    // Composite: (expr1) AND (expr2) or (expr1) OR (expr2)
    if trimmed.starts_with('(') {
        let mut depth = 0;
        let mut close_paren = None;
        let mut split_pos = None;
        let mut op_type: Option<&str> = None;
        for (i, ch) in trimmed.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && close_paren.is_none() {
                        close_paren = Some(i);
                    }
                }
                _ => {}
            }
            if depth == 0 && close_paren.is_some() {
                let remaining = &trimmed[i + 1..];
                if remaining.starts_with(" AND ") {
                    split_pos = Some(i + 1);
                    op_type = Some("AND");
                    break;
                }
                if remaining.starts_with(" OR ") {
                    split_pos = Some(i + 1);
                    op_type = Some("OR");
                    break;
                }
            }
        }
        if let Some(pos) = split_pos {
            let close = close_paren.ok_or_else(|| {
                crate::error::AegisError::SchemaValidation(format!(
                    "unmatched opening parenthesis in condition: {:?}",
                    expr
                ))
            })?;
            let left_str = trimmed[1..close].trim();
            let offset = if op_type == Some("OR") { 4 } else { 5 };
            let right_str = trimmed[pos + offset..].trim();
            let right_str = right_str
                .strip_prefix('(')
                .and_then(|s| s.strip_suffix(')'))
                .unwrap_or(right_str);
            let left = parse_condition(left_str)?;
            let right = parse_condition(right_str)?;
            return match op_type {
                Some("AND") => Ok(ConditionExpr::And(vec![left, right])),
                Some("OR") => Ok(ConditionExpr::Or(vec![left, right])),
                _ => unreachable!(),
            };
        }
    }

    // Leaf condition: "attr op value"
    let parts: Vec<&str> = trimmed.splitn(2, char::is_whitespace).collect();
    if parts.len() < 2 {
        return Err(crate::error::AegisError::SchemaValidation(format!(
            "invalid condition expression: {:?}",
            expr
        )));
    }
    let attr = parts[0].to_string();
    let rest = parts[1].trim().to_string();

    if let Some(val) = rest.strip_prefix("eq ") {
        Ok(ConditionExpr::Leaf {
            attr,
            op: ConditionOp::Eq(val.trim().to_string()),
        })
    } else if let Some(val) = rest.strip_prefix("neq ") {
        Ok(ConditionExpr::Leaf {
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
        Ok(ConditionExpr::Leaf {
            attr,
            op: ConditionOp::In(items),
        })
    } else if rest == "exists" {
        Ok(ConditionExpr::Leaf {
            attr,
            op: ConditionOp::Exists,
        })
    } else if rest == "not_exists" {
        Ok(ConditionExpr::Leaf {
            attr,
            op: ConditionOp::NotExists,
        })
    } else if let Some(val) = rest.strip_prefix("gt ") {
        Ok(ConditionExpr::Leaf {
            attr,
            op: ConditionOp::Gt(val.trim().to_string()),
        })
    } else if let Some(val) = rest.strip_prefix("lt ") {
        Ok(ConditionExpr::Leaf {
            attr,
            op: ConditionOp::Lt(val.trim().to_string()),
        })
    } else if let Some(val) = rest.strip_prefix("before ") {
        Ok(ConditionExpr::Leaf {
            attr,
            op: ConditionOp::Before(val.trim().to_string()),
        })
    } else if let Some(val) = rest.strip_prefix("after ") {
        Ok(ConditionExpr::Leaf {
            attr,
            op: ConditionOp::After(val.trim().to_string()),
        })
    } else if let Some(val) = rest.strip_prefix("day_of_week ") {
        let items: Vec<String> = val
            .trim()
            .trim_matches(|c| c == '[' || c == ']')
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        Ok(ConditionExpr::Leaf {
            attr,
            op: ConditionOp::DayOfWeek(items),
        })
    } else {
        Err(crate::error::AegisError::SchemaValidation(format!(
            "unknown condition operator in: {:?}",
            expr
        )))
    }
}

fn evaluate_leaf(attr: &str, op: &ConditionOp, ctx: &ConditionEvalContext) -> bool {
    let value = ctx
        .subject_meta
        .get(attr)
        .or_else(|| ctx.resource_meta.get(attr))
        .or_else(|| ctx.env.get(attr));

    match op {
        ConditionOp::Eq(expected) => value == Some(expected),
        ConditionOp::Neq(expected) => value != Some(expected),
        ConditionOp::In(items) => value.is_some_and(|v| items.contains(v)),
        ConditionOp::Exists => value.is_some(),
        ConditionOp::NotExists => value.is_none(),
        ConditionOp::Gt(expected) => value
            .and_then(|v| v.parse::<f64>().ok())
            .zip(expected.parse::<f64>().ok())
            .is_some_and(|(v, e)| v > e),
        ConditionOp::Lt(expected) => value
            .and_then(|v| v.parse::<f64>().ok())
            .zip(expected.parse::<f64>().ok())
            .is_some_and(|(v, e)| v < e),
        ConditionOp::Before(time_str) => {
            let now = Utc::now();
            let parsed = parse_time(time_str);
            parsed.is_some_and(|t| now < t)
        }
        ConditionOp::After(time_str) => {
            let now = Utc::now();
            let parsed = parse_time(time_str);
            parsed.is_some_and(|t| now > t)
        }
        ConditionOp::DayOfWeek(days) => {
            let now = Utc::now();
            let today = match now.weekday() {
                chrono::Weekday::Mon => "Mon",
                chrono::Weekday::Tue => "Tue",
                chrono::Weekday::Wed => "Wed",
                chrono::Weekday::Thu => "Thu",
                chrono::Weekday::Fri => "Fri",
                chrono::Weekday::Sat => "Sat",
                chrono::Weekday::Sun => "Sun",
            };
            days.iter().any(|d| d == today)
        }
    }
}

fn parse_time(time_str: &str) -> Option<chrono::DateTime<Utc>> {
    // Try ISO 8601 first
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(time_str) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(time_str, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.and_utc());
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(time_str, "%Y-%m-%d %H:%M:%S") {
        return Some(dt.and_utc());
    }
    // Try HH:MM format — use today's date
    if let Ok(naive_time) = chrono::NaiveTime::parse_from_str(time_str, "%H:%M") {
        let today = Utc::now().date_naive();
        let naive_dt = today.and_time(naive_time);
        return Some(naive_dt.and_utc());
    }
    None
}

pub fn evaluate_condition(expr: &ConditionExpr, ctx: &ConditionEvalContext) -> bool {
    match expr {
        ConditionExpr::Leaf { attr, op } => evaluate_leaf(attr, op, ctx),
        ConditionExpr::And(exprs) => exprs.iter().all(|e| evaluate_condition(e, ctx)),
        ConditionExpr::Or(exprs) => exprs.iter().any(|e| evaluate_condition(e, ctx)),
        ConditionExpr::Not(inner) => !evaluate_condition(inner, ctx),
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
    fn test_not_exists_attr_missing() {
        let expr = parse_condition("missing not_exists").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_not_exists_attr_present() {
        let expr = parse_condition("role not_exists").unwrap();
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

    #[test]
    fn test_and_composite() {
        let expr = parse_condition("(role eq admin) AND (score gt 90)").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_and_one_false() {
        let expr = parse_condition("(role eq admin) AND (score lt 50)").unwrap();
        assert!(!evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_or_composite() {
        let expr = parse_condition("(role eq viewer) OR (score gt 90)").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_or_all_false() {
        let expr = parse_condition("(role eq viewer) OR (score lt 50)").unwrap();
        assert!(!evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_not_composite() {
        let expr = parse_condition("NOT (role eq viewer)").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_not_false() {
        let expr = parse_condition("NOT (role eq admin)").unwrap();
        assert!(!evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_before_iso() {
        // "before" a future date should be true
        let expr = parse_condition("attr before 2099-01-01T00:00:00Z").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_after_iso() {
        // "after" a past date should be true
        let expr = parse_condition("attr after 2020-01-01T00:00:00Z").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_before_past() {
        // "before" a past date should be false
        let expr = parse_condition("attr before 2020-01-01T00:00:00Z").unwrap();
        assert!(!evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_day_of_week() {
        let now = Utc::now();
        let today = match now.weekday() {
            chrono::Weekday::Mon => "Mon",
            chrono::Weekday::Tue => "Tue",
            chrono::Weekday::Wed => "Wed",
            chrono::Weekday::Thu => "Thu",
            chrono::Weekday::Fri => "Fri",
            chrono::Weekday::Sat => "Sat",
            chrono::Weekday::Sun => "Sun",
        };
        let expr_str = format!("attr day_of_week [{}]", today);
        let expr = parse_condition(&expr_str).unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_day_of_week_no_match() {
        let expr = parse_condition("attr day_of_week [Nonexistent]").unwrap();
        assert!(!evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_not_exists_in_ctx() {
        let expr = parse_condition("nonexistent_attr not_exists").unwrap();
        assert!(evaluate_condition(&expr, &ctx()));
    }

    #[test]
    fn test_time_hhmm_format() {
        let expr = parse_condition("attr after 00:00").unwrap();
        // 00:00 is always in the past for any reasonable test run
        assert!(evaluate_condition(&expr, &ctx()));
    }
}
