//! 知识库存储集成测试。
//!
//! 需要本地 PostgreSQL + pg-vec 扩展 + 运行过迁移。
//! 运行方式：
//!   PG_TEST_URL="postgres://user:pass@localhost:5432/devops_agent_test" \
//!   cargo test knowledge_store -- --include-ignored --nocapture

use sqlx::{PgPool, Row};

/// 测试 $1::vector 显式 cast 查询是否正常工作。
///
/// 前提：knowledge_entries 表中至少有一条 embedding 不为 NULL 的记录。
#[tokio::test]
#[ignore]
async fn test_vector_similarity_search() {
    let url = std::env::var("PG_TEST_URL")
        .expect("需要设置 PG_TEST_URL 环境变量");
    let pool = PgPool::connect(&url).await.expect("无法连接 PostgreSQL");

    // 插入测试数据（10 维向量，与生产维度不同，仅测试 SQL 语法）
    sqlx::query(
        "INSERT INTO knowledge_entries \
         (fingerprint, error_text, solution, category, confidence, embedding) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind("test-vector-fp")
    .bind("test error for vector search")
    .bind("test solution")
    .bind("compile")
    .bind(0.8_f32)
    .bind("0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8,0.9,1.0")
    .execute(&pool)
    .await
    .expect("插入测试数据失败");

    // 执行向量相似度查询（使用 $1::vector 显式 cast）
    let rows = sqlx::query(
        "SELECT id, fingerprint, solution, confidence \
         FROM knowledge_entries \
         WHERE embedding IS NOT NULL AND confidence > 0.3 \
         ORDER BY embedding <=> $1::vector \
         LIMIT 3",
    )
    .bind("0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8,0.9,1.0")
    .fetch_all(&pool)
    .await
    .expect("向量检索查询失败");

    // 验证返回了结果
    assert!(!rows.is_empty(), "向量检索应返回结果");

    let row = &rows[0];
    let fingerprint: String = row.get("fingerprint");
    assert_eq!(fingerprint, "test-vector-fp", "应返回刚插入的条目");

    // 清理
    sqlx::query("DELETE FROM knowledge_entries WHERE fingerprint = $1")
        .bind("test-vector-fp")
        .execute(&pool)
        .await
        .ok();

    println!("✅ test_vector_similarity_search PASSED");
}

/// 测试指纹精确查找。
#[tokio::test]
#[ignore]
async fn test_insert_and_find_by_fingerprint() {
    let url = std::env::var("PG_TEST_URL")
        .expect("需要设置 PG_TEST_URL 环境变量");
    let pool = PgPool::connect(&url).await.expect("无法连接 PostgreSQL");

    let fp = format!("test-fp-{}", std::process::id());

    // 插入
    let result = sqlx::query(
        "INSERT INTO knowledge_entries (fingerprint, error_text, solution, category, confidence) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(&fp)
    .bind("test error text")
    .bind("test solution")
    .bind("compile")
    .bind(0.5_f32)
    .fetch_one(&pool)
    .await
    .expect("插入失败");

    let id: i32 = result.get(0);

    // 按指纹查找
    let row = sqlx::query(
        "SELECT id, fingerprint, solution, confidence \
         FROM knowledge_entries \
         WHERE fingerprint = $1 AND confidence > 0.3 AND expires_at > now() \
         ORDER BY confidence DESC LIMIT 1",
    )
    .bind(&fp)
    .fetch_one(&pool)
    .await
    .expect("查找失败");

    assert_eq!(row.get::<i32, _>("id"), id);
    assert_eq!(row.get::<String, _>("fingerprint"), fp);
    assert_eq!(row.get::<String, _>("solution"), "test solution");

    // 清理
    sqlx::query("DELETE FROM knowledge_entries WHERE fingerprint = $1")
        .bind(&fp)
        .execute(&pool)
        .await
        .ok();

    println!("✅ test_insert_and_find_by_fingerprint PASSED");
}

/// 测试过期条目清理。
#[tokio::test]
#[ignore]
async fn test_cleanup_expired() {
    let url = std::env::var("PG_TEST_URL")
        .expect("需要设置 PG_TEST_URL 环境变量");
    let pool = PgPool::connect(&url).await.expect("无法连接 PostgreSQL");

    let expired_fp = format!("expired-fp-{}", std::process::id());
    let valid_fp = format!("valid-fp-{}", std::process::id());

    // 插入过期条目
    sqlx::query(
        "INSERT INTO knowledge_entries (fingerprint, error_text, solution, category, confidence, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&expired_fp)
    .bind("expired error")
    .bind("expired solution")
    .bind("other")
    .bind(0.5_f32)
    .bind(chrono::Utc::now() - chrono::Duration::days(1))
    .execute(&pool)
    .await
    .expect("插入过期条目失败");

    // 插入未过期条目
    sqlx::query(
        "INSERT INTO knowledge_entries (fingerprint, error_text, solution, category, confidence) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&valid_fp)
    .bind("valid error")
    .bind("valid solution")
    .bind("other")
    .bind(0.5_f32)
    .execute(&pool)
    .await
    .expect("插入有效条目失败");

    // 执行清理
    let deleted = sqlx::query("DELETE FROM knowledge_entries WHERE expires_at <= now()")
        .execute(&pool)
        .await
        .expect("清理失败")
        .rows_affected();

    assert!(deleted >= 1, "应至少删除 1 条过期记录，实际删除: {}", deleted);

    // 验证未过期条目仍然存在
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM knowledge_entries WHERE fingerprint = $1",
    )
    .bind(&valid_fp)
    .fetch_one(&pool)
    .await
    .expect("查询失败");

    assert_eq!(count, 1, "未过期条目应仍然存在");

    // 清理
    sqlx::query("DELETE FROM knowledge_entries WHERE fingerprint = $1 OR fingerprint = $2")
        .bind(&expired_fp)
        .bind(&valid_fp)
        .execute(&pool)
        .await
        .ok();

    println!("✅ test_cleanup_expired PASSED");
}
