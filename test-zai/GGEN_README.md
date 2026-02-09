# ggen CONSTRUCT Prompt Generation

Ontology-driven generation of CONSTRUCT prompts for A2A agents using ggen.

## 📁 Structure

```
test-zai/
├── ggen.toml                    # ggen configuration
├── ontologies/                  # RDF/Turtle ontologies
│   ├── agents.ttl              # Agent persona definitions
│   └── tools.ttl               # Tool definitions
├── queries/                     # SPARQL queries
│   ├── all_agents.rq           # Extract all agents
│   ├── tool_agents.rq          # Extract tool-enabled agents
│   └── dialogue_agents.rq      # Extract dialogue agents
├── templates/                   # Tera templates
│   ├── construct_base.tera     # CONSTRUCT prompt template
│   └── agent_module.rs.tera    # Rust agent module template
└── generated/                   # Generated output (gitignored)
    ├── prompts/                # Generated CONSTRUCT prompts
    └── agents/                 # Generated Rust agent modules
```

## 🔧 Usage

### 1. Define Agents in Ontology

Edit `ontologies/agents.ttl` to define new agent personas:

```turtle
a2a:CustomAgent a a2a:ToolAgent ;
    a2a:agentId "custom_agent" ;
    rdfs:label "Custom Agent" ;
    a2a:hasRole "Your custom role" ;
    a2a:hasDescription "Agent description" ;
    a2a:maxCharacters 200 ;
    a2a:sentenceCount "2-3 sentences" ;
    a2a:hasBehavioralRule "First behavioral rule" ;
    a2a:hasBehavioralRule "Second behavioral rule" ;
    a2a:outputFormat "Expected output format" ;
    a2a:hasTool a2a:SomeTool ;
    a2a:usesModel "zai-coding::glm-4.6" .
```

### 2. Query Agents with SPARQL

SPARQL queries extract structured data from the ontology:

**`queries/all_agents.rq`**: Get all agents with their properties
- Returns: agent_id, label, role, description, constraints, tools, model

**`queries/tool_agents.rq`**: Get only tool-enabled agents
- Filters for `a2a:ToolAgent` type
- Includes tool information

**`queries/dialogue_agents.rq`**: Get only dialogue agents
- Filters for `a2a:DialogueAgent` type
- Optimized for conversational agents

### 3. Generate with Tera Templates

Templates use SPARQL query results to generate output:

**`templates/construct_base.tera`**: Generate CONSTRUCT prompts
```
CONSTRUCT: {{ description }}
CONSTRAINTS:
{% for rule in behavioral_rules | split(pat="|") -%}
- {{ rule }}
{% endfor -%}
- Exactly {{ sentence_count }} per response
- Maximum {{ max_chars }} characters total
OUTPUT: {{ output_format }}
```

**`templates/agent_module.rs.tera`**: Generate Rust agent modules
- Creates a complete Rust struct with methods
- Embeds the CONSTRUCT prompt
- Ready to use with genai crate

### 4. Run ggen sync

```bash
# Generate all outputs from ontology
ggen sync

# Or with ggen installed:
cargo install ggen
ggen sync
```

This will:
1. Load ontologies from `ontologies/*.ttl`
2. Execute SPARQL queries from `queries/*.rq`
3. Apply Tera templates from `templates/*.tera`
4. Output to `generated/`

## 📊 Example Agents

Predefined in `ontologies/agents.ttl`:

### Tool Agents
- **MathAgent**: Mathematical computation with calculate tool
- **ResearcherAgent**: Information retrieval with search tool
- **DataAnalystAgent**: Data analysis with database and analysis tools

### Dialogue Agents
- **SocratesAgent**: Socratic questioning method
- **StudentAgent**: Thoughtful learning and inquiry
- **TeacherAgent**: Patient explanation and teaching

### Debate Agents
- **OptimistDebater**: Pro-technology advocate
- **CautiousDebater**: Risk-aware AI advocate
- **ModeratorAgent**: Impartial debate facilitator

## 🎯 Generated Outputs

### CONSTRUCT Prompts (`generated/prompts/`)
```
generated/prompts/
├── math_agent_prompt.txt
├── researcher_agent_prompt.txt
├── socrates_prompt.txt
└── ...
```

Each prompt file contains a ready-to-use CONSTRUCT prompt:
```
CONSTRUCT: Mathematical computation assistant with bounded operations
CONSTRAINTS:
- Use calculate tool for ALL numeric operations
- Show calculation then explain
- No approximations, exact values only
- Exactly 2-3 sentences per response
- Maximum 150 characters total
TOOLS: Calculate
OUTPUT: Tool result + brief explanation
```

### Rust Agent Modules (`generated/agents/`)
```
generated/agents/
├── math_agent.rs
├── researcher_agent.rs
├── socrates.rs
└── ...
```

Each module is a complete Rust implementation:
```rust
pub struct MathAgent {
    name: String,
    persona: String,
    model: String,
    history: Vec<ChatMessage>,
}

impl MathAgent {
    pub fn new() -> Self { /* ... */ }
    pub async fn respond(&mut self, client: &Client, message: &str)
        -> Result<String, Box<dyn std::error::Error>> { /* ... */ }
}
```

## 🔮 Benefits

### vs. Hardcoded Prompts
- ✅ Personas are **data**, not code
- ✅ Single source of truth in ontology
- ✅ Easy to version control and diff
- ✅ Can share ontologies across projects
- ✅ Semantic queries find agents by capability

### vs. Manual Generation
- ✅ **Consistent** formatting via templates
- ✅ **DRY**: Define once, generate many outputs
- ✅ **Type-safe**: Templates catch errors early
- ✅ **Maintainable**: Update ontology, regenerate all

### Ontology-Driven Development
- 🧬 Semantic reasoning about agents
- 🔍 SPARQL queries for agent discovery
- 🔗 Link agents to capabilities and tools
- 📦 Package ontologies as reusable libraries
- 🌐 Integrate with knowledge graphs

## 📖 Related

- [ggen](https://github.com/seanchatmangpt/ggen) - Ontology-driven code generation
- [SPARQL](https://www.w3.org/TR/sparql11-query/) - RDF query language
- [Tera](https://keats.github.io/tera/) - Template engine
- [RDF Turtle](https://www.w3.org/TR/turtle/) - Ontology format

## 🚀 Next Steps

1. Add more agent personas to ontology
2. Create domain-specific tool ontologies
3. Write custom SPARQL queries for agent discovery
4. Build template libraries for different frameworks
5. Integrate with knowledge graphs for dynamic agent composition
