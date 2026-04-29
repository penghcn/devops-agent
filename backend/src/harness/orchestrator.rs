use super::{Hook, HookPoint};
use anyhow;
use std::sync::Arc;
use async_trait::async_trait;

/// Step trait — 编排器执行的最小单元
#[async_trait]
pub trait Step: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self) -> anyhow::Result<String>;
}

/// Orchestrator — 编排核心，管理 Hook 注册与执行
pub struct Orchestrator {
    hooks: Vec<Arc<dyn Hook>>,
}

impl Orchestrator {
    pub fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    pub fn add_hook(&mut self, hook: Arc<dyn Hook>) {
        self.hooks.push(hook);
    }

    async fn fire(&self, point: HookPoint) -> anyhow::Result<()> {
        for hook in &self.hooks {
            hook.on(point).await?;
        }
        Ok(())
    }

    /// 执行步骤链，按 SessionStart → StepStart → StepEnd → SessionEnd 顺序触发 Hook
    pub async fn run(&self, steps: &[Arc<dyn Step>]) -> anyhow::Result<Vec<String>> {
        self.fire(HookPoint::SessionStart).await?;
        let mut results = Vec::new();

        for step in steps {
            self.fire(HookPoint::StepStart).await?;
            match step.execute().await {
                Ok(output) => {
                    results.push(output);
                    self.fire(HookPoint::StepEnd).await?;
                }
                Err(e) => {
                    self.fire(HookPoint::StepEnd).await?;
                    return Err(e);
                }
            }
        }

        self.fire(HookPoint::SessionEnd).await?;
        Ok(results)
    }
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}
