//! Prints the JSON Schema for `AgentConfig` to stdout.
//!
//! Used by the Terraform provider build to bundle a schema fixture:
//!
//! ```sh
//! cargo run -p a2a-agents --example print_schema --features schema -- > schema.json
//! ```

fn main() {
    use a2a_agents::core::AgentConfig;
    use schemars::schema_for;
    let schema = schema_for!(AgentConfig);
    println!("{}", serde_json::to_string_pretty(&schema).unwrap());
}
