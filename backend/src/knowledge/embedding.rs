//! 远程 Embedding API 调用。
//!
//! 调用阿里云 DashScope text-embedding-v3 模型。
//! 返回逗号分隔的向量字符串，适配 pg-vec 格式。

use reqwest::Client;

/// 调用远程 Embedding API，返回逗号分隔的向量字符串（pg-vec 格式）
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

    // 提取第一个向量，转为逗号分隔字符串（pg-vec 格式）
    body.get("output")
        .and_then(|o| o.get("embeddings"))
        .and_then(|e| e.as_array())
        .and_then(|arr| arr.first())
        .and_then(|emb| emb.get("embedding"))
        .and_then(|vec| {
            vec.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_f64())
                    .map(|v| format!("{:.6}", v as f32))
                    .collect::<Vec<_>>()
                    .join(",")
            })
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_long_text() {
        let long = "a".repeat(3000);
        let truncated = truncate_for_embedding(&long);
        assert_eq!(truncated.chars().count(), 1500);
    }

    #[test]
    fn test_truncate_short_text() {
        let short = "short error";
        let truncated = truncate_for_embedding(short);
        assert_eq!(truncated, short);
    }

    #[test]
    fn test_truncate_unicode() {
        let text = "编译错误".repeat(500); // 2000 chars
        let truncated = truncate_for_embedding(&text);
        assert_eq!(truncated.chars().count(), 1500);
    }

    #[test]
    fn test_embedding_returns_none_for_empty_key() {
        // get_embedding returns None when api_key is empty
        // This is a behavioral check, not a network test
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            get_embedding(&Client::new(), "test", "").await
        });
        assert!(result.is_none(), "空 API key 应返回 None");
    }
}
