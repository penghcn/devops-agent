---
name: devops-deploy
description: 部署服务到指定环境
---

# DevOps Deploy Skill

## 触发条件
用户要求部署服务时使用此 Skill。

## 执行流程
1. 从用户输入中提取：服务名称、环境（dev/staging/prod）、镜像版本
2. 调用 `./scripts/trigger_jenkins.sh` 触发部署 Job
3. 返回部署 ID 和状态

## 示例
用户："部署 order-service 到 staging"
→ 提取 {service: "order-service", env: "staging"}
→ 执行 ./scripts/trigger_jenkins.sh order-service staging
→ 返回 "部署已触发，Job ID: 123"