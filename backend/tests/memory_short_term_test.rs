use devops_agent::memory::{MemoryType, ShortTermMemory};

#[test]
fn short_term_memory_new_with_capacity() {
    let memory = ShortTermMemory::new(3);
    assert_eq!(memory.len(), 0);
    assert!(memory.is_empty());
}

#[test]
fn short_term_memory_add_and_entries() {
    let mut memory = ShortTermMemory::new(10);

    memory.add("first entry".to_string(), MemoryType::UserInput);
    assert_eq!(memory.len(), 1);
    assert_eq!(memory.entries().len(), 1);

    memory.add("second entry".to_string(), MemoryType::ToolCall);
    assert_eq!(memory.len(), 2);

    // Verify entries content
    let entries = memory.entries();
    assert_eq!(entries[0].content, "first entry");
    assert_eq!(entries[0].r#type, MemoryType::UserInput);
    assert_eq!(entries[1].content, "second entry");
    assert_eq!(entries[1].r#type, MemoryType::ToolCall);
}

#[test]
fn short_term_memory_eviction_fifo() {
    let mut memory = ShortTermMemory::new(3);

    memory.add("oldest".to_string(), MemoryType::UserInput);
    memory.add("middle".to_string(), MemoryType::ToolCall);
    memory.add("newest".to_string(), MemoryType::LlmResponse);

    assert_eq!(memory.len(), 3);

    // Add one more - should evict oldest
    memory.add("overflow".to_string(), MemoryType::ToolResult);
    assert_eq!(memory.len(), 3);

    let entries = memory.entries();
    assert_eq!(entries[0].content, "middle");
    assert_eq!(entries[1].content, "newest");
    assert_eq!(entries[2].content, "overflow");
}

#[test]
fn short_term_memory_recent() {
    let mut memory = ShortTermMemory::new(10);

    for i in 0..5 {
        memory.add(format!("entry {}", i), MemoryType::UserInput);
    }

    let recent = memory.recent(3);
    assert_eq!(recent.len(), 3);
    assert_eq!(recent[0].content, "entry 2");
    assert_eq!(recent[1].content, "entry 3");
    assert_eq!(recent[2].content, "entry 4");
}

#[test]
fn short_term_memory_clear() {
    let mut memory = ShortTermMemory::new(10);

    memory.add("entry 1".to_string(), MemoryType::UserInput);
    memory.add("entry 2".to_string(), MemoryType::ToolCall);
    assert_eq!(memory.len(), 2);

    memory.clear();
    assert_eq!(memory.len(), 0);
    assert!(memory.is_empty());
}

#[test]
fn short_term_memory_default_capacity() {
    let mut memory = ShortTermMemory::default();
    assert_eq!(memory.len(), 0);
    // Default capacity should be 200
    for _ in 0..200 {
        memory.add("test".to_string(), MemoryType::UserInput);
    }
    assert_eq!(memory.len(), 200);
}

#[test]
fn short_term_memory_entry_id_increment() {
    let mut memory = ShortTermMemory::new(10);

    memory.add("first".to_string(), MemoryType::UserInput);
    memory.add("second".to_string(), MemoryType::ToolCall);

    let entries = memory.entries();
    assert_eq!(entries[0].id, 1);
    assert_eq!(entries[1].id, 2);
}
