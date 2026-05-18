use agent_lib::tools::calculator::CalculatorTool;
use agent_lib::tools::r#trait::Tool;
use serde_json::json;

#[tokio::test]
async fn test_calculator_addition() {
    let tool = CalculatorTool::new();
    let result = tool.execute(json!({ "expression": "2 + 3" })).await.unwrap();
    assert_eq!(result["result"].as_f64().unwrap(), 5.0);
}

#[tokio::test]
async fn test_calculator_subtraction() {
    let tool = CalculatorTool::new();
    let result = tool.execute(json!({ "expression": "10 - 4" })).await.unwrap();
    assert_eq!(result["result"].as_f64().unwrap(), 6.0);
}

#[tokio::test]
async fn test_calculator_multiplication() {
    let tool = CalculatorTool::new();
    let result = tool.execute(json!({ "expression": "3 * 4" })).await.unwrap();
    assert_eq!(result["result"].as_f64().unwrap(), 12.0);
}

#[tokio::test]
async fn test_calculator_division() {
    let tool = CalculatorTool::new();
    let result = tool.execute(json!({ "expression": "15 / 3" })).await.unwrap();
    assert_eq!(result["result"].as_f64().unwrap(), 5.0);
}

#[tokio::test]
async fn test_calculator_parentheses() {
    let tool = CalculatorTool::new();
    let result = tool.execute(json!({ "expression": "(1 + 2) * 3" })).await.unwrap();
    assert_eq!(result["result"].as_f64().unwrap(), 9.0);
}

#[tokio::test]
async fn test_calculator_negative() {
    let tool = CalculatorTool::new();
    let result = tool.execute(json!({ "expression": "-5 + 3" })).await.unwrap();
    assert_eq!(result["result"].as_f64().unwrap(), -2.0);
}

#[tokio::test]
async fn test_calculator_division_by_zero() {
    let tool = CalculatorTool::new();
    let result = tool.execute(json!({ "expression": "10 / 0" })).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_calculator_empty_expression() {
    let tool = CalculatorTool::new();
    let result = tool.execute(json!({ "expression": "" })).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_calculator_missing_parameter() {
    let tool = CalculatorTool::new();
    let result = tool.execute(json!({})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_calculator_decimal() {
    let tool = CalculatorTool::new();
    let result = tool.execute(json!({ "expression": "3.5 + 2.5" })).await.unwrap();
    assert_eq!(result["result"].as_f64().unwrap(), 6.0);
}

#[tokio::test]
async fn test_calculator_complex_expression() {
    let tool = CalculatorTool::new();
    let result = tool.execute(json!({ "expression": "2 + 3 * 4" })).await.unwrap();
    assert_eq!(result["result"].as_f64().unwrap(), 14.0);
}
