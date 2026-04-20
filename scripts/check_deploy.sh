#!/bin/bash
# 检查 Jenkins 构建状态
# 用法: ./check_deploy.sh <job_name> [build_number]
set -euo pipefail

JOB_NAME="${1:?用法: check_deploy.sh <job_name> [build_number]}"
BUILD_NUMBER="${2:-last}"

JENKINS_URL="${JENKINS_URL:?请设置 JENKINS_URL 环境变量}"
JENKINS_USER="${JENKINS_USER:?请设置 JENKINS_USER 环境变量}"
JENKINS_TOKEN="${JENKINS_TOKEN:?请设置 JENKINS_TOKEN 环境变量}"

BUILD_ID="${BUILD_NUMBER == 'last' && 'lastBuild' || 'build/' + BUILD_NUMBER}"

echo "查询构建状态: ${JOB_NAME}#${BUILD_NUMBER}"

RESPONSE=$(curl -s -u "${JENKINS_USER}:${JENKINS_TOKEN}" \
  "${JENKINS_URL}/job/${JOB_NAME}/${BUILD_ID}/api/json?fields=id,result,color,building")

echo "$RESPONSE" | python3 -m json.tool 2>/dev/null || echo "$RESPONSE"

RESULT=$(echo "$RESPONSE" | python3 -c "import sys,json; print(json.load(sys.stdin).get('color','unknown'))" 2>/dev/null || echo "unknown")

case "$RESULT" in
  blue|green|success) echo "构建成功"; exit 0 ;;
  red|failed) echo "构建失败"; exit 1 ;;
  yellow|不稳定) echo "构建不稳定"; exit 1 ;;
  *) echo "构建状态: ${RESULT}"; exit 1 ;;
esac
