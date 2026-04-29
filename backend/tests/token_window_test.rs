use devops_agent::token::{ContextWindow, Layer};

#[test]
fn context_window_add_messages() {
    let mut window = ContextWindow::new(4096);

    window.add_to_layer(Layer::System, "You are a helpful assistant".to_string());
    window.add_to_layer(Layer::Linear, "User message 1".to_string());

    assert!(window.total_tokens() > 0);
}

#[test]
fn context_window_over_threshold() {
    let mut window = ContextWindow::new(100);

    // Add a large message that exceeds the window
    window.add_to_layer(
        Layer::Linear,
        "x".repeat(200), // token estimate: 200/4 = 50 tokens
    );

    assert!(!window.is_over_threshold(90.0));

    // Add more to exceed
    window.add_to_layer(
        Layer::Linear,
        "y".repeat(400), // token estimate: 400/4 = 100 tokens
    );

    assert!(window.is_over_threshold(50.0));
}

#[test]
fn context_window_compress_linear() {
    let mut window = ContextWindow::new(4096);

    for i in 0..10 {
        window.add_to_layer(Layer::Linear, format!("message {}", i));
    }

    // Compress to keep only last 3
    window.compress_linear(3);

    let context = window.build_context();
    // Verify linear messages are reduced
    let linear_msgs: Vec<&str> = context
        .iter()
        .filter(|m| m.starts_with("message "))
        .map(|s| s.as_str())
        .collect();
    assert_eq!(linear_msgs.len(), 3);
}

#[test]
fn context_window_compress_structured() {
    let mut window = ContextWindow::new(4096);

    window.add_to_layer(Layer::Structured, "structured 1".to_string());
    window.add_to_layer(Layer::Structured, "structured 2".to_string());
    window.add_to_layer(Layer::Linear, "linear msg".to_string());

    window.compress_structured();

    let context = window.build_context();
    // Structured messages should be gone
    assert!(!context.iter().any(|m| m.starts_with("structured")));
    // Linear should still be there
    assert!(context.iter().any(|m| m == "linear msg"));
}

#[test]
fn context_window_build_context_order() {
    let mut window = ContextWindow::new(4096);

    window.add_to_layer(Layer::Linear, "linear".to_string());
    window.add_to_layer(Layer::System, "system".to_string());
    window.add_to_layer(Layer::Compressed, "compressed".to_string());
    window.add_to_layer(Layer::Structured, "structured".to_string());

    let context = window.build_context();
    // Order: system -> compressed -> structured -> linear
    assert_eq!(context[0], "system");
    assert_eq!(context[1], "compressed");
    assert_eq!(context[2], "structured");
    assert_eq!(context[3], "linear");
}

#[test]
fn context_window_usage_percent() {
    let mut window = ContextWindow::new(1000);

    window.add_to_layer(Layer::Linear, "test message for percentage".to_string());

    let percent = window.usage_percent();
    assert!(percent > 0.0);
    assert!(percent < 100.0);
}
