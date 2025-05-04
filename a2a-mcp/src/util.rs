//! Utility functions for a2a-mcp integration

use crate::error::Result;

/// Ensures that a URL is properly formatted
pub fn validate_url(url: &str) -> Result<String> {
    let parsed = url::Url::parse(url)
        .map_err(|e| crate::error::Error::Translation(format!("Invalid URL: {}", e)))?;
    
    // Ensure URL has a scheme and host
    if parsed.scheme().is_empty() || parsed.host_str().is_none() {
        return Err(crate::error::Error::Translation("URL must have a scheme and host".into()));
    }
    
    // Normalize URL
    Ok(parsed.to_string())
}

/// Extracts tool name from an RMCP method string
/// For example: "https://example.com/agent:toolName" -> "toolName"
pub fn extract_tool_name(method: &str) -> Option<String> {
    method.split(':').last().map(|s| s.to_string())
}

/// Extracts agent URL from an RMCP method string
/// For example: "https://example.com/agent:toolName" -> "https://example.com/agent"
pub fn extract_agent_url(method: &str) -> Option<String> {
    method.split(':').next().map(|s| s.to_string())
}

/// Normalizes a task ID to ensure it's valid
pub fn normalize_task_id(task_id: &str) -> String {
    // If task_id is not a valid UUID, generate a new one
    if uuid::Uuid::parse_str(task_id).is_err() {
        return uuid::Uuid::new_v4().to_string();
    }
    task_id.to_string()
}