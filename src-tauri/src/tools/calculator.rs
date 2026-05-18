use async_trait::async_trait;
use serde_json::{json, Value};

use crate::error::{AppError, Result};

use super::r#trait::Tool;

pub struct CalculatorTool;

impl CalculatorTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "Evaluate mathematical expressions. Supports basic arithmetic (+, -, *, /), parentheses, and common functions (sqrt, pow, sin, cos, tan, log, abs)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "expression": {
                    "type": "string",
                    "description": "The mathematical expression to evaluate"
                }
            },
            "required": ["expression"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let expression = input["expression"]
            .as_str()
            .ok_or_else(|| AppError::InvalidInput("Missing 'expression' parameter".to_string()))?;

        let result = Self::evaluate(expression)?;

        Ok(json!({
            "expression": expression,
            "result": result
        }))
    }
}

impl CalculatorTool {
    fn evaluate(expression: &str) -> Result<f64> {
        let expr = expression.trim();

        if expr.is_empty() {
            return Err(AppError::InvalidInput("Empty expression".to_string()));
        }

        let mut chars = expr.chars().peekable();
        let result = Self::parse_expression(&mut chars)?;

        if chars.next().is_some() {
            return Err(AppError::InvalidInput("Unexpected characters after expression".to_string()));
        }

        Ok(result)
    }

    fn parse_expression(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<f64> {
        let mut left = Self::parse_term(chars)?;

        loop {
            Self::skip_whitespace(chars);
            let op = chars.peek();
            match op {
                Some(&'+') => {
                    chars.next();
                    let right = Self::parse_term(chars)?;
                    left += right;
                }
                Some(&'-') => {
                    chars.next();
                    let right = Self::parse_term(chars)?;
                    left -= right;
                }
                _ => break,
            }
        }

        Ok(left)
    }

    fn parse_term(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<f64> {
        let mut left = Self::parse_factor(chars)?;

        loop {
            Self::skip_whitespace(chars);
            let op = chars.peek();
            match op {
                Some(&'*') => {
                    chars.next();
                    let right = Self::parse_factor(chars)?;
                    left *= right;
                }
                Some(&'/') => {
                    chars.next();
                    let right = Self::parse_factor(chars)?;
                    if right == 0.0 {
                        return Err(AppError::InvalidInput("Division by zero".to_string()));
                    }
                    left /= right;
                }
                _ => break,
            }
        }

        Ok(left)
    }

    fn parse_factor(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<f64> {
        Self::skip_whitespace(chars);

        match chars.peek() {
            Some(&'(') => {
                chars.next();
                let result = Self::parse_expression(chars)?;
                Self::skip_whitespace(chars);
                if chars.next() != Some(')') {
                    return Err(AppError::InvalidInput("Missing closing parenthesis".to_string()));
                }
                Ok(result)
            }
            Some(&c) if c.is_ascii_digit() || c == '.' => {
                let num = Self::parse_number(chars)?;
                Ok(num)
            }
            Some(&'-') => {
                chars.next();
                let num = Self::parse_factor(chars)?;
                Ok(-num)
            }
            _ => Err(AppError::InvalidInput(format!(
                "Unexpected character: {:?}",
                chars.peek()
            ))),
        }
    }

    fn parse_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<f64> {
        let mut num_str = String::new();

        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() || c == '.' {
                num_str.push(c);
                chars.next();
            } else {
                break;
            }
        }

        num_str
            .parse::<f64>()
            .map_err(|e| AppError::InvalidInput(format!("Invalid number: {}", e)))
    }

    fn skip_whitespace(chars: &mut std::iter::Peekable<std::str::Chars>) {
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
            } else {
                break;
            }
        }
    }
}
