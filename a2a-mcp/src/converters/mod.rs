//! Protocol converters between A2A and MCP

pub mod llm_tool;
pub mod message;
pub mod skill_tool;
pub mod task_result;

pub use message::MessageConverter;
pub use skill_tool::{
    SKILL_SCHEMA_EXTENSION_URI, SkillSchema, SkillSchemas, SkillToolConverter, TASK_ID_PROPERTY,
};
pub use task_result::TaskResultConverter;
