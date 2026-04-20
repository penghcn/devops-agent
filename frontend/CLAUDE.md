# 架构
BUN + TS + Vite + Vue 3 + tailwindcss 前端

# 编译 测试
```
"scripts": {
    "dev": "bun run --bun vite --debug",
    "g": "bun run src/svg-generator-usage.ts",
    "build": "bun run --bun vite build",
    "preview": "bun run --bun vite preview",
    "fmt": "prettier --write .",
    "fmt:check": "prettier --check ."
  },
```