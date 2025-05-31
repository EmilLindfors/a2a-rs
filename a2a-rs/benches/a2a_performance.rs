//! A2A Protocol Performance Benchmarks
//!
//! Measures performance of core A2A operations including message serialization,
//! task operations, and streaming functionality to establish baseline metrics.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use a2a_rs::{
    adapter::SimpleAgentInfo,
    domain::{Message, Part, Task, TaskState, AgentSkill},
    MessageSendParams,
    application::SendMessageRequest,
};
use base64::Engine;
use serde_json::{self};

/// Benchmark message serialization performance
fn bench_message_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_serialization");
    
    // Test different message complexities
    let message_sizes = [1, 5, 10, 50];
    
    for &num_parts in message_sizes.iter() {
        // Create message with multiple parts
        let mut message = Message::user_text(
            "Performance test message with varying complexity".to_string(),
            format!("perf-test-{}", uuid::Uuid::new_v4())
        );
        
        // Add text parts
        for i in 1..num_parts {
            let text_part = Part::Text {
                text: format!("Additional text part {} with some content to test serialization performance", i),
                metadata: None,
            };
            message.add_part(text_part);
        }
        
        // Add a data part
        if num_parts > 2 {
            let mut data = serde_json::Map::new();
            for i in 0..10 {
                data.insert(format!("key_{}", i), serde_json::Value::Number(i.into()));
            }
            let data_part = Part::Data { data, metadata: None };
            message.add_part(data_part);
        }
        
        // Add a file part
        if num_parts > 3 {
            let file_data = vec![0u8; 1024]; // 1KB of data
            let encoded = base64::engine::general_purpose::STANDARD.encode(&file_data);
            let file_part = Part::file_from_bytes(
                encoded,
                Some("test_file.bin".to_string()),
                Some("application/octet-stream".to_string()),
            );
            message.add_part(file_part);
        }
        
        group.throughput(Throughput::Elements(num_parts as u64));
        
        // Benchmark serialization
        group.bench_with_input(
            BenchmarkId::new("serialize", num_parts),
            &message,
            |b, msg| {
                b.iter(|| {
                    let serialized = serde_json::to_value(black_box(msg)).unwrap();
                    black_box(serialized)
                })
            },
        );
        
        // Benchmark deserialization
        let serialized = serde_json::to_value(&message).unwrap();
        group.bench_with_input(
            BenchmarkId::new("deserialize", num_parts),
            &serialized,
            |b, data| {
                b.iter(|| {
                    let msg: Message = serde_json::from_value(black_box(data.clone())).unwrap();
                    black_box(msg)
                })
            },
        );
        
        // Benchmark roundtrip
        group.bench_with_input(
            BenchmarkId::new("roundtrip", num_parts),
            &message,
            |b, msg| {
                b.iter(|| {
                    let serialized = serde_json::to_value(black_box(msg)).unwrap();
                    let deserialized: Message = serde_json::from_value(serialized).unwrap();
                    black_box(deserialized)
                })
            },
        );
    }
    
    group.finish();
}

/// Benchmark task operations performance
fn bench_task_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("task_operations");
    
    // Benchmark task creation
    group.bench_function("task_creation", |b| {
        b.iter(|| {
            let task_id = format!("bench-{}", uuid::Uuid::new_v4());
            let context_id = format!("ctx-{}", uuid::Uuid::new_v4());
            let task = Task::new(black_box(task_id), black_box(context_id));
            black_box(task)
        })
    });
    
    // Benchmark task status updates
    let mut task = Task::new("bench-task".to_string(), "bench-context".to_string());
    let message = Message::user_text("Update message".to_string(), "msg-1".to_string());
    
    group.bench_function("status_update", |b| {
        b.iter(|| {
            task.update_status(black_box(TaskState::Working), black_box(Some(message.clone())));
        })
    });
    
    // Benchmark task history truncation with different limits
    let history_limits = [1, 5, 10, 50, 100];
    
    // First, create a task with substantial history
    let mut task_with_history = Task::new("history-task".to_string(), "history-context".to_string());
    for i in 0..100 {
        let msg = Message::user_text(format!("Message {}", i), format!("msg-{}", i));
        task_with_history.update_status(TaskState::Working, Some(msg));
    }
    
    for &limit in history_limits.iter() {
        group.bench_with_input(
            BenchmarkId::new("history_truncation", limit),
            &limit,
            |b, &lim| {
                b.iter(|| {
                    let limited = black_box(&task_with_history).with_limited_history(Some(lim));
                    black_box(limited)
                })
            },
        );
    }
    
    group.finish();
}

/// Benchmark JSON-RPC request handling
fn bench_jsonrpc_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("jsonrpc_operations");
    
    // Create sample JSON-RPC request
    let message = Message::user_text("JSON-RPC test message".to_string(), "jsonrpc-msg".to_string());
    let params = MessageSendParams {
        message,
        configuration: None,
        metadata: None,
    };
    
    let request = SendMessageRequest {
        jsonrpc: "2.0".to_string(),
        method: "message/send".to_string(),
        id: Some(serde_json::Value::String("req-123".to_string())),
        params,
    };
    
    // Benchmark JSON-RPC request serialization
    group.bench_function("request_serialize", |b| {
        b.iter(|| {
            let serialized = serde_json::to_value(black_box(&request)).unwrap();
            black_box(serialized)
        })
    });
    
    // Benchmark JSON-RPC request deserialization
    let serialized_request = serde_json::to_value(&request).unwrap();
    group.bench_function("request_deserialize", |b| {
        b.iter(|| {
            let req: SendMessageRequest = serde_json::from_value(black_box(serialized_request.clone())).unwrap();
            black_box(req)
        })
    });
    
    group.finish();
}

/// Benchmark part validation and processing
fn bench_part_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("part_operations");
    
    // Benchmark text part creation
    group.bench_function("text_part_creation", |b| {
        b.iter(|| {
            let part = Part::Text {
                text: black_box("Performance test text content".to_string()),
                metadata: None,
            };
            black_box(part)
        })
    });
    
    // Benchmark data part creation
    group.bench_function("data_part_creation", |b| {
        b.iter(|| {
            let mut data = serde_json::Map::new();
            data.insert("key1".to_string(), serde_json::Value::String("value1".to_string()));
            data.insert("key2".to_string(), serde_json::Value::Number(42.into()));
            let part = Part::Data { data: black_box(data), metadata: None };
            black_box(part)
        })
    });
    
    // Benchmark file part creation with different sizes
    let file_sizes = [100, 1024, 10240, 102400]; // 100B, 1KB, 10KB, 100KB
    
    for &size in file_sizes.iter() {
        let file_data = vec![0u8; size];
        let encoded = base64::engine::general_purpose::STANDARD.encode(&file_data);
        
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(
            BenchmarkId::new("file_part_creation", size),
            &encoded,
            |b, data| {
                b.iter(|| {
                    let part = Part::file_from_bytes(
                        black_box(data.clone()),
                        Some("test.bin".to_string()),
                        Some("application/octet-stream".to_string()),
                    );
                    black_box(part)
                })
            },
        );
    }
    
    // Benchmark part validation
    let text_part = Part::Text {
        text: "Valid text content".to_string(),
        metadata: None,
    };
    
    group.bench_function("part_validation", |b| {
        b.iter(|| {
            // Simulate validation by checking part properties
            match black_box(&text_part) {
                Part::Text { text, .. } => !text.is_empty(),
                Part::Data { data, .. } => !data.is_empty(),
                Part::File { file, .. } => file.bytes.is_some() || file.uri.is_some(),
            }
        })
    });
    
    group.finish();
}

/// Benchmark agent card operations
fn bench_agent_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("agent_operations");
    
    // Benchmark agent info creation
    group.bench_function("agent_info_creation", |b| {
        b.iter(|| {
            let agent_info = SimpleAgentInfo::new(
                black_box("Benchmark Agent".to_string()),
                black_box("http://localhost:8080".to_string()),
            )
            .with_description("Performance test agent".to_string())
            .with_version("1.0.0".to_string())
            .with_streaming()
            .with_push_notifications()
            .with_state_transition_history();
            
            black_box(agent_info)
        })
    });
    
    // Benchmark skill addition
    group.bench_function("skill_addition", |b| {
        b.iter(|| {
            let agent_info = SimpleAgentInfo::new(
                "Skill Test Agent".to_string(),
                "http://localhost:8080".to_string(),
            );
            let result = black_box(agent_info).add_skill(
                black_box(format!("skill-{}", uuid::Uuid::new_v4())),
                black_box("Test Skill".to_string()),
                black_box(Some("A test skill for benchmarking".to_string())),
            );
            black_box(result)
        })
    });
    
    // Benchmark agent skill serialization
    let skill = AgentSkill {
        id: "test-skill".to_string(),
        name: "Test Skill".to_string(),
        description: "A benchmark skill".to_string(),
        tags: vec!["test".to_string(), "benchmark".to_string()],
        examples: Some(vec!["example1".to_string(), "example2".to_string()]),
        input_modes: Some(vec!["text/plain".to_string()]),
        output_modes: Some(vec!["text/plain".to_string()]),
    };
    
    group.bench_function("skill_serialization", |b| {
        b.iter(|| {
            let serialized = serde_json::to_value(black_box(&skill)).unwrap();
            black_box(serialized)
        })
    });
    
    group.finish();
}

/// Benchmark memory usage patterns
fn bench_memory_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_operations");
    
    // Benchmark message cloning
    let complex_message = {
        let mut msg = Message::user_text(
            "Complex message for memory benchmarking".to_string(),
            "memory-test-msg".to_string()
        );
        
        // Add multiple parts
        for i in 0..10 {
            let text_part = Part::Text {
                text: format!("Text part {} with substantial content for memory testing", i),
                metadata: None,
            };
            msg.add_part(text_part);
        }
        
        // Add data part
        let mut data = serde_json::Map::new();
        for i in 0..50 {
            data.insert(format!("key_{}", i), serde_json::Value::String(format!("value_{}", i)));
        }
        msg.add_part(Part::Data { data, metadata: None });
        
        msg
    };
    
    group.bench_function("message_clone", |b| {
        b.iter(|| {
            let cloned = black_box(&complex_message).clone();
            black_box(cloned)
        })
    });
    
    // Benchmark task history memory usage
    group.bench_function("history_memory_usage", |b| {
        b.iter(|| {
            let mut task = Task::new("memory-task".to_string(), "memory-context".to_string());
            
            // Add many messages to test memory allocation
            for i in 0..100 {
                let msg = Message::user_text(
                    format!("History message {} for memory testing", i),
                    format!("hist-msg-{}", i)
                );
                task.update_status(TaskState::Working, Some(msg));
            }
            
            black_box(task)
        })
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_message_serialization,
    bench_task_operations,
    bench_jsonrpc_operations,
    bench_part_operations,
    bench_agent_operations,
    bench_memory_operations
);

criterion_main!(benches);