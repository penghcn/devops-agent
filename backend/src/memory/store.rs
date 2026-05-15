use sqlx::{Row, SqlitePool};

/// SQLite 记忆存储层，负责数据库操作
#[derive(Debug)]
pub struct MemoryStore {
    pool: SqlitePool,
}

impl MemoryStore {
    /// 创建或打开 SQLite 数据库，初始化 memories 表
    pub async fn new(path: &str) -> sqlx::Result<Self> {
        let pool = SqlitePool::connect(path).await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                type TEXT NOT NULL,
                keywords TEXT NOT NULL DEFAULT '',
                score REAL NOT NULL DEFAULT 1.0,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    /// 插入记忆条目
    pub async fn insert(
        &self,
        content: &str,
        type_: &str,
        keywords: &[&str],
        score: f64,
    ) -> sqlx::Result<i64> {
        let keywords_str = keywords.join(",");
        let created_at = chrono::Local::now().to_rfc3339();

        let result = sqlx::query(
            "INSERT INTO memories (content, type, keywords, score, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(content)
        .bind(type_)
        .bind(keywords_str)
        .bind(score)
        .bind(created_at)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// 按关键词 LIKE 搜索，按 score 降序排序，最多返回 20 条
    pub async fn search(&self, keyword: &str) -> sqlx::Result<Vec<String>> {
        let pattern = format!("%{}%", keyword);
        let rows = sqlx::query(
            "SELECT content FROM memories
             WHERE keywords LIKE ?1
             ORDER BY score DESC
             LIMIT 20",
        )
        .bind(&pattern)
        .map(|row: sqlx::sqlite::SqliteRow| row.get(0))
        .fetch_all(&self.pool)
        .await?;

        Ok(rows)
    }

    /// 返回总条目数
    pub async fn count(&self) -> sqlx::Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) FROM memories")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get(0))
    }
}
