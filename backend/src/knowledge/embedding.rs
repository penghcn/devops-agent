//! 远程 Embedding API 调用。
//!
//! 调用阿里云 DashScope text-embedding-v3 模型。

use reqwest::Client;

/// 调用远程 Embedding API，返回向量 JSON 字符串
pub async fn get_embedding(client: &Client, text: &str, api_key: &str) -> Option<String> {
    if api_key.is_empty() {
        return None;
    }

    let url =
        "https://dashscope.aliyuncs.com/api/v1/services/embeddings/text-embedding/text-embedding";

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .body(
            serde_json::json!({
                "model": "text-embedding-v3",
                "input": {
                    "texts": [truncate_for_embedding(text)]
                },
                "parameters": {
                    "dimensions": 768,
                    "encoding_format": "float"
                }
            })
            .to_string(),
        )
        .send()
        .await
        .ok()?;

    let body: serde_json::Value = resp.json().await.ok()?;

    // 提取第一个向量
    body.get("output")
        .and_then(|o| o.get("embeddings"))
        .and_then(|e| e.as_array())
        .and_then(|arr| arr.first())
        .and_then(|emb| emb.get("embedding"))
        .and_then(|vec| vec.to_string().into())
}

/// 截断文本以适应 Embedding API 限制（~512 token）
fn truncate_for_embedding(text: &str) -> String {
    let max_chars = 1500;
    if text.chars().count() > max_chars {
        text.chars().take(max_chars).collect()
    } else {
        text.to_string()
    }
}
