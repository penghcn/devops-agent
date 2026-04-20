#!/bin/bash
# 触发 Jenkins 构建任务
# 用法: ./trigger_jenkins.sh <job_name> [env] [version]
set -euo pipefail

JOB_NAME="${1:?用法: trigger_jenkins.sh <job_name> [env] [version]}"
ENV="${2:-dev}"
VERSION="${3:-latest}"

# 从环境变量读取 Jenkins 配置
JENKINS_URL="${JENKINS_URL:?请设置 JENKINS_URL 环境变量}"
JENKINS_USER="${JENKINS_USER:?请设置 JENKINS_USER 环境变量}"
JENKINS_TOKEN="${JENKINS_TOKEN:?请设置 JENKINS_TOKEN 环境变量}"

echo "触发 Jenkins 构建: ${JOB_NAME}"
echo "环境: ${ENV}, 版本: ${VERSION}"

RESPONSE=$(curl -s -o /dev/null -w "%{http_code}" \
  -u "${JENKINS_USER}:${JENKINS_TOKEN}" \
  -X POST "${JENKINS_URL}/job/${JOB_NAME}/buildWithParameters" \
  -d "env=${ENV}&version=${VERSION}")

if [ "$RESPONSE" = "200" ] || [ "$RESPONSE" = "201" ]; then
  echo "构建已触发，状态码: ${RESPONSE}"
else
  echo "构建触发失败，状态码: ${RESPONSE}"
  exit 1
fi
