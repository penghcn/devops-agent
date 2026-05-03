use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::config::Config;
use crate::tools::jenkins::JobTypeInfo;
use base64::Engine;

/// Jenkins Job 信息（缓存用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedJob {
    pub name: String,
    pub job_type: String,
    pub url: String,
    pub branches: Vec<String>,
}

/// Jenkins 缓存数据
#[derive(Debug, Clone, Serialize)]
pub struct JenkinsCache {
    pub jobs: Vec<CachedJob>,
    pub last_refresh: String,
}

/// 缓存管理器
pub struct JenkinsCacheManager {
    config: Config,
    cache: Arc<RwLock<Option<JenkinsCache>>>,
    #[expect(dead_code)]
    refresh_interval: Duration,
}

impl JenkinsCacheManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            cache: Arc::new(RwLock::new(None)),
            refresh_interval: Duration::from_secs(60),
        }
    }

    pub fn cache(&self) -> Arc<RwLock<Option<JenkinsCache>>> {
        self.cache.clone()
    }

    /// 异步刷新缓存
    pub async fn refresh(&self) -> Result<()> {
        let jobs = self.fetch_all_jobs().await?;
        let cache = JenkinsCache {
            jobs,
            last_refresh: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        };
        *self.cache.write().await = Some(cache);
        Ok(())
    }

    /// 获取缓存（如果过期则刷新）
    pub async fn get_cached(&self) -> Option<JenkinsCache> {
        let cache = self.cache.read().await;
        if let Some(ref c) = *cache {
            return Some(c.clone());
        }
        drop(cache);

        // 缓存为空，先刷新
        self.refresh().await.ok();
        self.cache.read().await.clone()
    }

    /// 获取所有 Job 及分支
    async fn fetch_all_jobs(&self) -> Result<Vec<CachedJob>> {
        let client = reqwest::Client::new();
        let auth_value = format!("{}:{}", self.config.jenkins_user, self.config.jenkins_token);
        let encoded = base64::engine::general_purpose::STANDARD.encode(&auth_value);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Basic {}", encoded))?,
        );

        // 获取所有 Job
        let root_url = format!(
            "{}/api/json?fields=_class,name,url,jobs[_class,name,url]",
            self.config.jenkins_url
        );
        let response = client
            .get(&root_url)
            .headers(headers.clone())
            .send()
            .await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to list Jenkins jobs: {}", response.status());
        }

        let body: serde_json::Value = response.json().await?;
        let jobs = body
            .get("jobs")
            .and_then(|v| v.as_array())
            .unwrap_or(&vec![])
            .to_vec();

        let mut result = Vec::new();

        for job in jobs {
            let name = job
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let url = job
                .get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string();
            let class = job.get("_class").and_then(|c| c.as_str()).unwrap_or("");

            let job_type = JobTypeInfo::from_class(class);

            // 多分支 Pipeline 需要获取分支列表
            let branches = if matches!(job_type, JobTypeInfo::MultiBranchPipeline) {
                self.fetch_branches(&name, &headers)
                    .await
                    .unwrap_or_default()
            } else {
                vec![]
            };

            result.push(CachedJob {
                name,
                job_type: match job_type {
                    JobTypeInfo::MultiBranchPipeline => "pipeline_multibranch".to_string(),
                    JobTypeInfo::Pipeline => "pipeline".to_string(),
                    JobTypeInfo::Job => "job".to_string(),
                },
                url,
                branches,
            });
        }

        Ok(result)
    }

    /// 获取多分支 Pipeline 的分支列表
    async fn fetch_branches(
        &self,
        job_name: &str,
        headers: &reqwest::header::HeaderMap,
    ) -> Result<Vec<String>> {
        let client = reqwest::Client::new();
        let url = format!(
            "{}/job/{}/api/json?fields=jobs[name]",
            self.config.jenkins_url, job_name
        );

        let response = client.get(&url).headers(headers.clone()).send().await?;

        if !response.status().is_success() {
            return Ok(vec![]);
        }

        let body: serde_json::Value = response.json().await?;
        let branches = body
            .get("jobs")
            .and_then(|v| v.as_array())
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|j| j.get("name").and_then(|n| n.as_str()))
            .map(|s| s.to_string())
            .collect();

        Ok(branches)
    }

    /// 根据名称查找 Job
    pub async fn find_job(&self, name: &str) -> Option<CachedJob> {
        self.cache
            .read()
            .await
            .as_ref()
            .and_then(|c| c.jobs.iter().find(|j| j.name == name).cloned())
    }

    /// 判断 Job 是否存在（从缓存）
    pub async fn job_exists(&self, name: &str) -> bool {
        self.find_job(name).await.is_some()
    }

    /// 获取 Job 的分支列表（从缓存）
    pub async fn get_branches(&self, job_name: &str) -> Vec<String> {
        self.find_job(job_name)
            .await
            .map(|j| j.branches)
            .unwrap_or_default()
    }
}
