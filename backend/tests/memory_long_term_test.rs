use devops_agent::memory::{LongTermMemory, MemoryStore, MemoryType};
use std::process;
use tempfile::tempdir;

#[test]
fn memory_store_new_creates_table() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(format!("test_{}.db", process::id()));

    let store = MemoryStore::new(path.to_str().unwrap()).expect("should create store");
    assert_eq!(store.count().unwrap(), 0);
}

#[test]
fn memory_store_insert_and_count() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(format!("test_{}.db", process::id()));

    let store = MemoryStore::new(path.to_str().unwrap()).expect("should create store");
    assert_eq!(store.count().unwrap(), 0);

    store
        .insert("test content", "UserInput", &["keyword"], 1.0)
        .expect("should insert");
    assert_eq!(store.count().unwrap(), 1);

    store
        .insert("another content", "ToolCall", &["other"], 2.0)
        .expect("should insert");
    assert_eq!(store.count().unwrap(), 2);
}

#[test]
fn memory_store_search_by_keyword() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(format!("test_{}.db", process::id()));

    let store = MemoryStore::new(path.to_str().unwrap()).expect("should create store");

    store
        .insert("high score item", "Decision", &["important", "key1"], 5.0)
        .expect("should insert");
    store
        .insert("low score item", "UserInput", &["important", "key2"], 1.0)
        .expect("should insert");
    store
        .insert("no match item", "ToolCall", &["other"], 3.0)
        .expect("should insert");

    let results = store.search("important").expect("should search");
    assert_eq!(results.len(), 2);
    // Should be ordered by score DESC
    assert_eq!(results[0], "high score item");
    assert_eq!(results[1], "low score item");
}

#[test]
fn long_term_memory_new() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(format!("test_{}.db", process::id()));

    let memory = LongTermMemory::new(path.to_str().unwrap()).expect("should create");
    let results = memory.retrieve("anything").expect("should retrieve");
    assert!(results.is_empty());
}

#[test]
fn long_term_memory_save_and_retrieve() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(format!("test_{}.db", process::id()));

    let mut memory = LongTermMemory::new(path.to_str().unwrap()).expect("should create");

    memory
        .save(
            "deployment successful",
            MemoryType::ToolResult,
            &["deploy", "success"],
            3.0,
        )
        .expect("should save");
    memory
        .save(
            "build failed with error",
            MemoryType::ToolResult,
            &["build", "error"],
            5.0,
        )
        .expect("should save");

    // Retrieve by keyword
    let results = memory.retrieve("deploy").expect("should retrieve");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], "deployment successful");

    // Retrieve with no match
    let results = memory.retrieve("nonexistent").expect("should retrieve");
    assert!(results.is_empty());
}
