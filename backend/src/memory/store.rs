use rusqlite::{Connection, Result};

/// SQLite 记忆存储层，负责数据库操作
#[derive(Debug)]
pub struct MemoryStore {
    conn: Connection,
}

impl MemoryStore {
    /// 创建或打开 SQLite 数据库，初始化 memories 表
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                type TEXT NOT NULL,
                keywords TEXT NOT NULL DEFAULT '',
                score REAL NOT NULL DEFAULT 1.0,
                created_at TEXT NOT NULL
            )",
        )?;
        Ok(Self { conn })
    }

    /// 插入记忆条目
    pub fn insert(&self, content: &str, type_: &str, keywords: &[&str], score: f64) -> Result<i64> {
        let keywords_str = keywords.join(",");
        let created_at = chrono::Utc::now().to_rfc3339();

        self.conn.execute(
            "INSERT INTO memories (content, type, keywords, score, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (&content, &type_, &keywords_str, score, &created_at),
        )?;

        Ok(self.conn.last_insert_rowid())
    }

    /// 按关键词 LIKE 搜索，按 score 降序排序，最多返回 20 条
    pub fn search(&self, keyword: &str) -> Result<Vec<String>> {
        let pattern = format!("%{}%", keyword);
        let mut stmt = self.conn.prepare(
            "SELECT content FROM memories
             WHERE keywords LIKE ?1
             ORDER BY score DESC
             LIMIT 20",
        )?;

        let mut rows = stmt.query([&pattern])?;
        let mut results = Vec::new();

        while let Some(row) = rows.next()? {
            let content: String = row.get(0)?;
            results.push(content);
        }

        Ok(results)
    }

    /// 返回总条目数
    pub fn count(&self) -> Result<i64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))?;
        Ok(count)
    }
}
