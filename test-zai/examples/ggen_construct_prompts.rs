//! CONSTRUCT Prompt Generation using ggen-inspired patterns
//!
//! Demonstrates generating CONSTRUCT prompts dynamically from specifications
//! rather than hardcoding them.

use test_zai::construct_gen::{presets, ConstructGenerator, ConstraintSpec, ToolSpec};
use genai::Client;
use genai::chat::{ChatMessage, ChatRequest, Tool};
use serde_json::json;

#[derive(Clone)]
struct Agent {
    name: String,
    persona: String,
    model: String,
    history: Vec<ChatMessage>,
}

impl Agent {
    fn new(name: &str, persona: &str, model: &str) -> Self {
        Self {
            name: name.to_string(),
            persona: persona.to_string(),
            model: model.to_string(),
            history: vec![ChatMessage::system(persona)],
        }
    }

    async fn respond(
        &mut self,
        client: &Client,
        message: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        self.history.push(ChatMessage::user(message));

        let chat_req = ChatRequest::new(self.history.clone());
        let chat_res = client.exec_chat(&self.model, chat_req, None).await?;

        let response = chat_res.first_text().ok_or("No response")?.to_string();
        self.history.push(ChatMessage::assistant(&response));

        Ok(response)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🏗️  GGEN-INSPIRED CONSTRUCT PROMPT GENERATION\n");

    let zai_key = std::env::var("ZAI_API_KEY").unwrap_or_default();
    if zai_key.is_empty() {
        eprintln!("⚠️  Set ZAI_API_KEY to run this example");
        return Ok(());
    }

    let client = Client::default();
    let gen = ConstructGenerator::new()?;

    println!("{}", "=".repeat(70));
    println!("\n📋 PART 1: GENERATING CONSTRUCT PROMPTS\n");

    // Generate prompts using presets
    println!("🔧 Generating Math Agent prompt from preset...\n");
    let math_persona = presets::math_agent();
    let math_prompt = gen.generate(&math_persona)?;
    println!("Generated CONSTRUCT prompt:");
    println!("{}", "-".repeat(70));
    println!("{}", math_prompt);
    println!("{}", "-".repeat(70));

    println!("\n🔍 Generating Researcher Agent prompt from preset...\n");
    let researcher_persona = presets::researcher_agent();
    let researcher_prompt = gen.generate(&researcher_persona)?;
    println!("Generated CONSTRUCT prompt:");
    println!("{}", "-".repeat(70));
    println!("{}", researcher_prompt);
    println!("{}", "-".repeat(70));

    println!("\n🎭 Generating Socrates Agent prompt from preset...\n");
    let socrates_persona = presets::socrates_agent();
    let socrates_prompt = gen.generate(&socrates_persona)?;
    println!("Generated CONSTRUCT prompt:");
    println!("{}", "-".repeat(70));
    println!("{}", socrates_prompt);
    println!("{}", "-".repeat(70));

    println!("\n👨‍🎓 Generating Student Agent prompt from preset...\n");
    let student_persona = presets::student_agent();
    let student_prompt = gen.generate(&student_persona)?;
    println!("Generated CONSTRUCT prompt:");
    println!("{}", "-".repeat(70));
    println!("{}", student_prompt);
    println!("{}", "-".repeat(70));

    println!("\n{}", "=".repeat(70));
    println!("\n📋 PART 2: CUSTOM CONSTRUCT GENERATION\n");

    // Generate a custom debate agent using the template
    let debate_constraints = ConstraintSpec {
        sentence_count: "3-4 sentences".to_string(),
        max_chars: 250,
        behavioral_rules: vec![
            "Emphasize scientific evidence".to_string(),
            "Reference peer-reviewed research".to_string(),
        ],
        output_format: "Evidence-based argument with citations".to_string(),
    };

    let custom_debate_prompt = gen.generate_debate_agent(
        "Climate science advocate with bounded scientific argumentation",
        &debate_constraints,
        "Emphasize climate science consensus",
        "Cite peer-reviewed studies and IPCC reports",
    )?;

    println!("🌍 Custom Climate Science Debater:");
    println!("{}", "-".repeat(70));
    println!("{}", custom_debate_prompt);
    println!("{}", "-".repeat(70));

    // Generate a custom tool agent
    let tool_constraints = ConstraintSpec {
        sentence_count: "2-3 sentences".to_string(),
        max_chars: 200,
        behavioral_rules: vec![
            "Always validate API responses".to_string(),
            "Report errors explicitly".to_string(),
        ],
        output_format: "API result with validation status".to_string(),
    };

    let custom_tool_prompt = gen.generate_tool_agent(
        "API integration specialist with error handling",
        &tool_constraints,
        &[
            ToolSpec {
                name: "call_api".to_string(),
                parameters: "endpoint, method, payload".to_string(),
            },
            ToolSpec {
                name: "validate_response".to_string(),
                parameters: "response, schema".to_string(),
            },
        ],
        "Use call_api for ALL external requests",
    )?;

    println!("\n🔌 Custom API Integration Agent:");
    println!("{}", "-".repeat(70));
    println!("{}", custom_tool_prompt);
    println!("{}", "-".repeat(70));

    println!("\n{}", "=".repeat(70));
    println!("\n📋 PART 3: USING GENERATED PROMPTS WITH REAL AGENTS\n");

    // Create agents using generated prompts
    let mut socrates = Agent::new(
        "Socrates",
        &socrates_prompt,
        "zai-coding::glm-4.6",
    );

    let mut student = Agent::new(
        "Student",
        &student_prompt,
        "zai-coding::glm-4.6",
    );

    // Have a brief conversation
    let topic = "What is the nature of knowledge?";
    println!("Topic: {}\n", topic);
    println!("{}", "=".repeat(70));

    let mut current_message = topic.to_string();
    let mut current_speaker = "Student";

    for turn in 0..4 {
        println!(
            "\n[Turn {}] {} → {}",
            turn + 1,
            current_speaker,
            if current_speaker == "Student" {
                "Socrates"
            } else {
                "Student"
            }
        );
        println!("{}", "-".repeat(70));
        println!("📨 {}", current_message);
        println!();

        let (responder, responder_name) = if current_speaker == "Student" {
            (&mut socrates, "Socrates")
        } else {
            (&mut student, "Student")
        };

        print!("🤖 {} thinking... ", responder_name);
        let response = responder.respond(&client, &current_message).await?;
        println!("done!");
        println!("💬 {}", response);

        current_message = response;
        current_speaker = responder_name;
    }

    println!("\n{}", "=".repeat(70));
    println!("✅ CONSTRUCT prompt generation complete!\n");

    println!("📊 SUMMARY:");
    println!("{}", "-".repeat(70));
    println!("✨ Generated prompts for:");
    println!("   • Math Agent (tool-enabled)");
    println!("   • Researcher Agent (tool-enabled)");
    println!("   • Socrates Agent (Socratic dialogue)");
    println!("   • Student Agent (dialogue participant)");
    println!("   • Custom Climate Debater (debate agent)");
    println!("   • Custom API Agent (tool-enabled)");
    println!("\n💡 Key Benefits:");
    println!("   • Prompts generated from specifications, not hardcoded");
    println!("   • Template-based consistency across agent types");
    println!("   • Easy to version control persona specs as data");
    println!("   • Enables ontology-driven agent generation (ggen pattern)");
    println!("\n🔮 Next Steps:");
    println!("   • Store persona specs in RDF/Turtle files");
    println!("   • Query specs with SPARQL for agent generation");
    println!("   • Create prompt libraries from ontologies");
    println!("   • Enable runtime prompt composition from semantic data");

    Ok(())
}
