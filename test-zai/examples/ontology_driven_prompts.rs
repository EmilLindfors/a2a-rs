//! Ontology-Driven CONSTRUCT Prompt Generation
//!
//! Full ggen pattern: Define agent personas in RDF, query with SPARQL,
//! generate CONSTRUCT prompts from semantic data.

use sophia_api::graph::Graph;
use sophia_api::ns::Namespace;
use sophia_api::prelude::Triple;
use sophia_api::term::Term;
use sophia_inmem::graph::LightGraph;
use sophia_turtle::parser::turtle;
use std::collections::HashMap;

// Define custom namespace for our agent ontology
const A2A: &str = "http://example.org/a2a#";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n🧬 ONTOLOGY-DRIVEN CONSTRUCT PROMPT GENERATION\n");
    println!("{}", "=".repeat(70));

    // Define agent ontology in Turtle format
    let ontology = r#"
@prefix a2a: <http://example.org/a2a#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .

# Agent Classes
a2a:Agent a rdfs:Class ;
    rdfs:label "AI Agent" ;
    rdfs:comment "An autonomous AI agent with a persona" .

a2a:ToolAgent a rdfs:Class ;
    rdfs:subClassOf a2a:Agent ;
    rdfs:label "Tool-Enabled Agent" ;
    rdfs:comment "An agent that can call tools" .

a2a:DialogueAgent a rdfs:Class ;
    rdfs:subClassOf a2a:Agent ;
    rdfs:label "Dialogue Agent" ;
    rdfs:comment "An agent designed for conversations" .

# Properties
a2a:hasRole a rdf:Property ;
    rdfs:domain a2a:Agent ;
    rdfs:range xsd:string ;
    rdfs:label "has role" .

a2a:hasDescription a rdf:Property ;
    rdfs:domain a2a:Agent ;
    rdfs:range xsd:string ;
    rdfs:label "has description" .

a2a:maxCharacters a rdf:Property ;
    rdfs:domain a2a:Agent ;
    rdfs:range xsd:integer ;
    rdfs:label "maximum characters" .

a2a:sentenceCount a rdf:Property ;
    rdfs:domain a2a:Agent ;
    rdfs:range xsd:string ;
    rdfs:label "sentence count" .

a2a:hasBehavioralRule a rdf:Property ;
    rdfs:domain a2a:Agent ;
    rdfs:range xsd:string ;
    rdfs:label "has behavioral rule" .

a2a:outputFormat a rdf:Property ;
    rdfs:domain a2a:Agent ;
    rdfs:range xsd:string ;
    rdfs:label "output format" .

a2a:hasTool a rdf:Property ;
    rdfs:domain a2a:ToolAgent ;
    rdfs:range a2a:Tool ;
    rdfs:label "has tool" .

# Agent Instances
a2a:MathAgent a a2a:ToolAgent ;
    rdfs:label "Math Agent" ;
    a2a:hasRole "Mathematical computation assistant" ;
    a2a:hasDescription "Mathematical computation assistant with bounded operations" ;
    a2a:maxCharacters 150 ;
    a2a:sentenceCount "2-3 sentences" ;
    a2a:hasBehavioralRule "Use calculate tool for ALL numeric operations" ;
    a2a:hasBehavioralRule "Show calculation then explain" ;
    a2a:hasBehavioralRule "No approximations, exact values only" ;
    a2a:outputFormat "Tool result + brief explanation" ;
    a2a:hasTool a2a:CalculateTool .

a2a:SocratesAgent a a2a:DialogueAgent ;
    rdfs:label "Socrates" ;
    a2a:hasRole "Socratic questioner" ;
    a2a:hasDescription "You are Socrates using the Socratic method" ;
    a2a:maxCharacters 200 ;
    a2a:sentenceCount "2-3 sentences" ;
    a2a:hasBehavioralRule "Ask exactly 1 probing question" ;
    a2a:hasBehavioralRule "Reference previous context" ;
    a2a:hasBehavioralRule "No explanations, only questions" ;
    a2a:outputFormat "One question that challenges assumptions" .

a2a:ResearcherAgent a a2a:ToolAgent ;
    rdfs:label "Researcher" ;
    a2a:hasRole "Information retrieval specialist" ;
    a2a:hasDescription "Information retrieval specialist with bounded search" ;
    a2a:maxCharacters 180 ;
    a2a:sentenceCount "2-3 sentences" ;
    a2a:hasBehavioralRule "Use search tool for information gathering" ;
    a2a:hasBehavioralRule "Cite tool results explicitly" ;
    a2a:hasBehavioralRule "Focus on key facts only" ;
    a2a:outputFormat "Synthesized facts from search results" ;
    a2a:hasTool a2a:SearchTool .

a2a:StudentAgent a a2a:DialogueAgent ;
    rdfs:label "Student" ;
    a2a:hasRole "Thoughtful student" ;
    a2a:hasDescription "You are a thoughtful student exploring ideas" ;
    a2a:maxCharacters 200 ;
    a2a:sentenceCount "2-3 sentences" ;
    a2a:hasBehavioralRule "Build on previous point" ;
    a2a:hasBehavioralRule "Ask 1 follow-up question" ;
    a2a:hasBehavioralRule "Show reasoning process" ;
    a2a:outputFormat "Insight + question that advances dialogue" .

# Tool Definitions
a2a:Tool a rdfs:Class ;
    rdfs:label "Agent Tool" .

a2a:CalculateTool a a2a:Tool ;
    rdfs:label "Calculate" ;
    a2a:toolName "calculate" ;
    a2a:toolParameters "a, b, operation" .

a2a:SearchTool a a2a:Tool ;
    rdfs:label "Search" ;
    a2a:toolName "search" ;
    a2a:toolParameters "query" .
"#;

    println!("\n📝 Loading Agent Ontology from Turtle...\n");
    println!("{}", "-".repeat(70));
    println!("{}", ontology);
    println!("{}", "-".repeat(70));

    // Parse the ontology
    let graph: LightGraph = turtle::parse_str(ontology).collect_triples()?;

    println!("\n✅ Ontology loaded: {} triples", graph.triples().count());

    println!("\n{}", "=".repeat(70));
    println!("\n🔍 QUERYING AGENT DEFINITIONS\n");

    // Query for all agents
    let agents = query_agents(&graph)?;

    println!("Found {} agents in ontology:", agents.len());
    for (agent_uri, agent_data) in &agents {
        println!("\n📋 Agent: {}", agent_uri);
        println!("   Role: {}", agent_data.get("role").unwrap_or(&"N/A".to_string()));
        println!("   Max Chars: {}", agent_data.get("maxChars").unwrap_or(&"N/A".to_string()));
        println!("   Sentence Count: {}", agent_data.get("sentenceCount").unwrap_or(&"N/A".to_string()));

        if let Some(rules) = agent_data.get("behavioralRules") {
            println!("   Behavioral Rules:");
            for rule in rules.split("|") {
                println!("      • {}", rule);
            }
        }
    }

    println!("\n{}", "=".repeat(70));
    println!("\n🏗️  GENERATING CONSTRUCT PROMPTS FROM ONTOLOGY\n");

    // Generate CONSTRUCT prompts for each agent
    for (agent_uri, agent_data) in &agents {
        let prompt = generate_construct_from_ontology(agent_data)?;

        println!("\n{} CONSTRUCT Prompt:", agent_uri);
        println!("{}", "-".repeat(70));
        println!("{}", prompt);
        println!("{}", "-".repeat(70));
    }

    println!("\n{}", "=".repeat(70));
    println!("\n✅ ONTOLOGY-DRIVEN GENERATION COMPLETE\n");

    println!("📊 SUMMARY:");
    println!("{}", "-".repeat(70));
    println!("✨ Demonstrated ggen pattern:");
    println!("   1. Define agent personas in RDF/Turtle ontology");
    println!("   2. Load ontology into semantic graph");
    println!("   3. Query graph for agent specifications");
    println!("   4. Generate CONSTRUCT prompts from semantic data");
    println!("\n💡 Benefits:");
    println!("   • Personas are data, not code");
    println!("   • Can query/filter agents semantically");
    println!("   • Version control ontology files");
    println!("   • Share ontologies across projects");
    println!("   • Reason about agent capabilities");
    println!("\n🔮 Next Steps:");
    println!("   • Add SPARQL query engine for complex queries");
    println!("   • Create ontology libraries for different domains");
    println!("   • Enable runtime agent discovery from ontologies");
    println!("   • Integrate with knowledge graphs");

    Ok(())
}

/// Query the graph for all agent definitions
fn query_agents(graph: &LightGraph) -> Result<HashMap<String, HashMap<String, String>>, Box<dyn std::error::Error>> {
    let mut agents: HashMap<String, HashMap<String, String>> = HashMap::new();

    let a2a_ns = Namespace::new(A2A)?;

    // Find all instances of a2a:Agent or its subclasses
    let agent_class = a2a_ns.get("Agent")?;
    let tool_agent_class = a2a_ns.get("ToolAgent")?;
    let dialogue_agent_class = a2a_ns.get("DialogueAgent")?;

    for triple in graph.triples() {
        let triple = triple?;
        let predicate = triple.p();

        if predicate.to_string().contains("type") {
            let object = triple.o();
            let obj_str = object.to_string();

            if obj_str.contains("Agent") {
                let subject = triple.s();
                let agent_uri = subject.to_string();

                if !agents.contains_key(&agent_uri) {
                    agents.insert(agent_uri.clone(), HashMap::new());
                }

                // Collect all properties for this agent
                for prop_triple in graph.triples() {
                    let prop_triple = prop_triple?;
                    if prop_triple.s().to_string() == agent_uri {
                        let pred_str = prop_triple.p().to_string();
                        let obj_str = prop_triple.o().to_string();

                        if pred_str.contains("hasRole") {
                            let role = extract_literal(&obj_str);
                            agents.get_mut(&agent_uri).unwrap().insert("role".to_string(), role);
                        } else if pred_str.contains("hasDescription") {
                            let desc = extract_literal(&obj_str);
                            agents.get_mut(&agent_uri).unwrap().insert("description".to_string(), desc);
                        } else if pred_str.contains("maxCharacters") {
                            let chars = extract_literal(&obj_str);
                            agents.get_mut(&agent_uri).unwrap().insert("maxChars".to_string(), chars);
                        } else if pred_str.contains("sentenceCount") {
                            let count = extract_literal(&obj_str);
                            agents.get_mut(&agent_uri).unwrap().insert("sentenceCount".to_string(), count);
                        } else if pred_str.contains("hasBehavioralRule") {
                            let rule = extract_literal(&obj_str);
                            let current = agents.get(&agent_uri).unwrap().get("behavioralRules").cloned().unwrap_or_default();
                            let new_rules = if current.is_empty() {
                                rule
                            } else {
                                format!("{}|{}", current, rule)
                            };
                            agents.get_mut(&agent_uri).unwrap().insert("behavioralRules".to_string(), new_rules);
                        } else if pred_str.contains("outputFormat") {
                            let format = extract_literal(&obj_str);
                            agents.get_mut(&agent_uri).unwrap().insert("outputFormat".to_string(), format);
                        }
                    }
                }
            }
        }
    }

    Ok(agents)
}

/// Extract literal value from RDF string representation
fn extract_literal(s: &str) -> String {
    // Simple extraction - in production use proper RDF parsing
    s.trim_matches('"')
        .split("^^")
        .next()
        .unwrap_or(s)
        .trim_matches('"')
        .to_string()
}

/// Generate CONSTRUCT prompt from ontology data
fn generate_construct_from_ontology(agent_data: &HashMap<String, String>) -> Result<String, Box<dyn std::error::Error>> {
    let role = agent_data.get("role").map(|s| s.as_str()).unwrap_or("Agent");
    let description = agent_data.get("description").map(|s| s.as_str()).unwrap_or(role);
    let max_chars = agent_data.get("maxChars").map(|s| s.as_str()).unwrap_or("200");
    let sentence_count = agent_data.get("sentenceCount").map(|s| s.as_str()).unwrap_or("2-3 sentences");
    let output_format = agent_data.get("outputFormat").map(|s| s.as_str()).unwrap_or("Bounded response");

    let mut prompt = format!("CONSTRUCT: {}\n", description);
    prompt.push_str("CONSTRAINTS:\n");

    if let Some(rules) = agent_data.get("behavioralRules") {
        for rule in rules.split("|") {
            prompt.push_str(&format!("- {}\n", rule));
        }
    }

    prompt.push_str(&format!("- Exactly {} per response\n", sentence_count));
    prompt.push_str(&format!("- Maximum {} characters total\n", max_chars));
    prompt.push_str(&format!("OUTPUT: {}", output_format));

    Ok(prompt)
}
