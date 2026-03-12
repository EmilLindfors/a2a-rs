//! AgentPlugin implementation for ReimbursementHandler
//!
//! This module provides the AgentPlugin trait implementation, which enables
//! automatic skill discovery and metadata provisioning.

use a2a_agents::traits::{AgentPlugin, SkillDefinition};
use async_trait::async_trait;

use crate::handler::ReimbursementHandler;

/// Implement AgentPlugin for ReimbursementHandler with InMemoryTaskStorage
#[async_trait]
impl<T> AgentPlugin for ReimbursementHandler<T>
where
    T: a2a_rs::port::AsyncTaskManager + Clone + Send + Sync + 'static,
{
    fn name(&self) -> &str {
        "Reimbursement Agent"
    }

    fn description(&self) -> &str {
        "Intelligent expense reimbursement assistant that helps users submit and track reimbursement requests through natural conversation"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn skills(&self) -> Vec<SkillDefinition> {
        vec![
            SkillDefinition {
                id: "submit_reimbursement".to_string(),
                name: "Submit Reimbursement Request".to_string(),
                description: "Guide users through submitting an expense reimbursement request".to_string(),
                keywords: vec![
                    "reimburse".into(),
                    "reimbursement".into(),
                    "expense".into(),
                    "receipt".into(),
                    "refund".into(),
                    "claim".into(),
                    "submit".into(),
                ],
                examples: vec![
                    "I need to submit a reimbursement".into(),
                    "I want to get reimbursed for an expense".into(),
                    "Submit expense claim".into(),
                ],
                input_formats: vec!["text".into(), "file".into()],
                output_formats: vec!["text".into(), "data".into()],
            },
            SkillDefinition {
                id: "track_request".to_string(),
                name: "Track Request Status".to_string(),
                description: "Check the status of existing reimbursement requests".to_string(),
                keywords: vec![
                    "status".into(),
                    "track".into(),
                    "check".into(),
                    "where is".into(),
                    "progress".into(),
                ],
                examples: vec![
                    "What's the status of my request?".into(),
                    "Check my reimbursement status".into(),
                ],
                input_formats: vec!["text".into()],
                output_formats: vec!["text".into(), "data".into()],
            },
            SkillDefinition {
                id: "help".to_string(),
                name: "Get Help".to_string(),
                description: "Provide information about the reimbursement process".to_string(),
                keywords: vec![
                    "help".into(),
                    "how".into(),
                    "what".into(),
                    "info".into(),
                    "information".into(),
                ],
                examples: vec![
                    "How do I submit a reimbursement?".into(),
                    "What information do I need?".into(),
                ],
                input_formats: vec!["text".into()],
                output_formats: vec!["text".into()],
            },
        ]
    }
}
