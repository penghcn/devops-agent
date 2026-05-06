#!/bin/bash
# 优雅重启前后端，先检测并优雅杀死之前的进程
# 用法: ./scripts/run.sh
set -euo pipefail

BIN_DIR="$HOME/data/app/"
SCRIPT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
LOG_DIR="$SCRIPT_DIR/logs"
PID_FILE_BACKEND="$LOG_DIR/backend.pid"
PID_FILE_FRONTEND="$LOG_DIR/frontend.pid"
PORT_FILE_BACKEND="$LOG_DIR/backend.port"
PORT_FILE_FRONTEND="$LOG_DIR/frontend.port"

mkdir -p "$LOG_DIR"

# 工具函数：优雅终止进程，超时则强制杀死
graceful_kill() {
  local pid_file="$1"
  local label="$2"

  if [[ ! -f "$pid_file" ]]; then
    echo "  $label 无旧进程"
    return 0
  fi

  local pid
  pid=$(cat "$pid_file")

  # 检查进程是否存活
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "  $label 进程已结束 (pid=$pid)，清理 pid 文件"
    rm -f "$pid_file"
    return 0
  fi

  echo "  $label 发现运行中进程 (pid=$pid)，发送 SIGTERM..."
  rm -f "$pid_file"
  kill "$pid" 2>/dev/null || true

  # 最多等待 10 秒
  for i in $(seq 1 10); do
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "  $label 已优雅退出"
      return 0
    fi
    sleep 1
  done

  # 超时强制杀死
  echo "  $label 未响应，强制 SIGKILL..."
  kill -9 "$pid" 2>/dev/null || true
  sleep 1
  echo "  $label 已强制终止"
}

echo "=== 检查旧进程 ==="
graceful_kill "$PID_FILE_BACKEND" "后端"
graceful_kill "$PID_FILE_FRONTEND" "前端"

# 清理残留的 devops-agent 进程
orphan_pids=$(pgrep -f "target/debug/devops-agent" 2>/dev/null || true)
if [[ -n "$orphan_pids" ]]; then
  echo "  清理残留后端进程: $orphan_pids"
  echo "$orphan_pids" | xargs kill 2>/dev/null || true
fi

echo ""
echo "=== 启动后端 ==="
cd "$SCRIPT_DIR/backend"

# 编译
echo "编译后端..."
cargo build 2>&1

BIN_PATH="$BIN_DIR/target/debug/devops-agent"

# Ad-hoc 签名 (macOS 必需)
echo "签名二进制文件..."
codesign --force --sign - "$BIN_PATH"

# 启动后端（后端会写 logs/backend.port 和 logs/frontend.port）
DEVOPS_LOG_DIR="$LOG_DIR" nohup "$BIN_PATH" >> "$LOG_DIR/backend.log" 2>&1 &
BACKEND_PID=$!
echo "$BACKEND_PID" > "$PID_FILE_BACKEND"
echo "后端已启动 (pid=$BACKEND_PID)"

# 等待后端就绪：读端口文件，再做健康检查
echo "等待后端就绪..."
BACKEND_PORT=""
FRONTEND_PORT=""
for i in $(seq 1 15); do
  if [[ -f "$PORT_FILE_BACKEND" ]]; then
    BACKEND_PORT=$(cat "$PORT_FILE_BACKEND" 2>/dev/null || echo "")
    FRONTEND_PORT=$(cat "$PORT_FILE_FRONTEND" 2>/dev/null || echo "")
    if [[ -n "$BACKEND_PORT" ]] && curl -sf "http://localhost:${BACKEND_PORT}/api/cache" >/dev/null 2>&1; then
      echo "后端就绪！(port=$BACKEND_PORT)"
      break
    fi
  fi
  if ! kill -0 "$BACKEND_PID" 2>/dev/null; then
    echo "错误: 后端进程已退出！查看 logs/backend.log"
    tail -30 "$LOG_DIR/backend.log"
    exit 1
  fi
  sleep 1
done

echo ""
echo "=== 启动前端 ==="
cd "$SCRIPT_DIR/frontend"
BACKEND_PORT="$BACKEND_PORT" FRONTEND_PORT="$FRONTEND_PORT" bun run dev &
FRONTEND_PID=$!
echo "$FRONTEND_PID" > "$PID_FILE_FRONTEND"
echo "前端已启动 (pid=$FRONTEND_PID)"

echo ""
echo "=== 运行中 ==="
echo "后端: pid=$BACKEND_PID port=$BACKEND_PORT (logs/backend.log)"
echo "前端: pid=$FRONTEND_PID port=$FRONTEND_PORT"
echo "按 Ctrl+C 停止"
echo ""

# 等待任意一个进程退出
trap 'echo ""; echo "收到中断信号，正在停止..."; kill $BACKEND_PID $FRONTEND_PID 2>/dev/null; rm -f "$PID_FILE_BACKEND" "$PID_FILE_FRONTEND"; echo "已停止."; exit' INT TERM

wait
