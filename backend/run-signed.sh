#!/bin/bash
# 编译项目
cargo build

# 获取二进制文件路径（根据你的项目名修改 `devops-agent`）
BIN_PATH="/Users/pengh/data/app/target/debug/devops-agent"

# 对二进制文件进行 Ad-hoc 签名
echo "Signing $BIN_PATH ..."
codesign --force --sign - "$BIN_PATH"

# 运行已签名的程序
echo "Running $BIN_PATH ..."
"$BIN_PATH"