use devops_agent::memory::{LongTermMemory, MemoryStore, MemoryType};

/// 使用共享内存数据库，避免 macOS 沙箱下文件路径问题
fn memory_url() -> String {
    "sqlite::memory:?cache=shared".to_string()
}

#[tokio::test]
async fn memory_store_new_creates_table() {
    let store = MemoryStore::new(&memory_url())
        .await
        .expect("should create store");
    assert_eq!(store.count().await.unwrap(), 0);
}

#[tokio::test]
async fn memory_store_insert_and_count() {
    let store = MemoryStore::new(&memory_url())
        .await
        .expect("should create store");
    assert_eq!(store.count().await.unwrap(), 0);

    store
        .insert("test content", "UserInput", &["keyword"], 1.0)
        .await
        .expect("should insert");
    assert_eq!(store.count().await.unwrap(), 1);

    store
        .insert("another content", "ToolCall", &["other"], 2.0)
        .await
        .expect("should insert");
    assert_eq!(store.count().await.unwrap(), 2);
}

#[tokio::test]
async fn memory_store_search_by_keyword() {
    let store = MemoryStore::new(&memory_url())
        .await
        .expect("should create store");

    store
        .insert("high score item", "Decision", &["important", "key1"], 5.0)
        .await
        .expect("should insert");
    store
        .insert("low score item", "UserInput", &["important", "key2"], 1.0)
        .await
        .expect("should insert");
    store
        .insert("no match item", "ToolCall", &["other"], 3.0)
        .await
        .expect("should insert");

    let results = store.search("important").await.expect("should search");
    assert_eq!(results.len(), 2);
    // Should be ordered by score DESC
    assert_eq!(results[0], "high score item");
    assert_eq!(results[1], "low score item");
}

#[tokio::test]
async fn long_term_memory_new() {
    let memory = LongTermMemory::new(&memory_url())
        .await
        .expect("should create");
    let results = memory.retrieve("anything").await.expect("should retrieve");
    assert!(results.is_empty());
}

#[tokio::test]
async fn long_term_memory_save_and_retrieve() {
    let memory = LongTermMemory::new(&memory_url())
        .await
        .expect("should create");

    memory
        .save(
            "deployment successful",
            MemoryType::ToolResult,
            &["deploy", "success"],
            3.0,
        )
        .await
        .expect("should save");
    memory
        .save(
            "build failed with error",
            MemoryType::ToolResult,
            &["build", "error"],
            5.0,
        )
        .await
        .expect("should save");

    // Retrieve by keyword
    let results = memory.retrieve("deploy").await.expect("should retrieve");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0], "deployment successful");

    // Retrieve with no match
    let results = memory
        .retrieve("nonexistent")
        .await
        .expect("should retrieve");
    assert!(results.is_empty());
}
