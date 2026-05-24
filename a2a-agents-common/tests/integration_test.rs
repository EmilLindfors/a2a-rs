use a2a_agents_common::{
    caching::AgentCache,
    formatting::{MarkdownFormatter, TableFormatter},
    nlp::{EntityExtractor, IntentClassifier},
};

#[tokio::test]
async fn test_agent_workflow() {
    // Test NLP components
    let classifier = IntentClassifier::new()
        .add_intent("query_weather", &["weather", "forecast", "temperature"])
        .add_intent("query_stock", &["stock", "price", "market"]);

    let extractor = EntityExtractor::new()
        .with_pattern("location", r"(new york|london|tokyo|sf)")
        .with_pattern("company", r"(apple|google|microsoft|amazon)");

    let input = "What is the weather in tokyo today?";
    
    let intent = classifier.classify(input);
    assert_eq!(intent, Some("query_weather"));

    let entities = extractor.extract(input);
    let locations = entities.get("location").unwrap();
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0], "tokyo");

    // Test Caching component
    let cache = AgentCache::<String, String>::new();
    cache.insert("weather:tokyo".to_string(), "Sunny, 25C".to_string()).await;

    let result = cache.get(&"weather:tokyo".to_string()).await.unwrap();
    assert_eq!(result, "Sunny, 25C");

    // Test Formatting component
    let md_table = TableFormatter::new()
        .header(&["City", "Weather"])
        .row(&["Tokyo", "Sunny, 25C"])
        .build();

    let output = MarkdownFormatter::new()
        .heading(1, "Weather Report")
        .paragraph(&format!("Here is the weather for {}:", locations[0]))
        .raw(&md_table)
        .build();

    println!("{}", output);

    assert!(output.contains("# Weather Report"));
    assert!(output.contains("Here is the weather for tokyo:"));
    assert!(output.contains("| City  | Weather    |"));
    assert!(output.contains("| Tokyo | Sunny, 25C |"));
}

#[tokio::test]
async fn test_error_handling() {
    let classifier = IntentClassifier::new()
        .add_intent("query_weather", &["weather", "forecast"])
        .add_intent("query_stock", &["stock", "price"]);

    let extractor = EntityExtractor::new()
        .with_pattern("location", r"(new york|london|tokyo)");

    let input = "Tell me a joke";
    
    let intent = classifier.classify(input);
    assert_eq!(intent, None);

    let entities = extractor.extract(input);
    assert!(!entities.contains_key("location"));
    
    // Formatting with missing data
    let output = MarkdownFormatter::new()
        .heading(1, "Error Report")
        .paragraph("Could not understand the intent or find any location.")
        .build();

    assert!(output.contains("# Error Report"));
}
