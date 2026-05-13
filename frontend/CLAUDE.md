# 架构
BUN + TS + Vite 8 + Vue 3.5 + Tailwind CSS 4 前端

# 依赖版本
- vue: ^3.5.0
- vite: ^8.0.0
- @vitejs/plugin-vue: ^6.0.0
- tailwindcss: ^4.1.0
- vue-tsc: ^3.0.0
- vitest: ^4.0.0
- typescript: ^5.8.0
- prettier: ^3.5.0

# Tailwind CSS v4 注意事项
- 无 `tailwind.config.js`，配置通过 CSS `@theme` 块
- 无 `postcss.config.js`，v4 内置 CSS 引擎
- CSS 入口：`@import "tailwindcss"`（不再是 `@tailwind` 指令）
- Vite 配置使用 `esbuild` CSS transformer 和 minifier（lightningcss 不支持 `@theme`）

# 编译 测试
```
"scripts": {
    "dev": "bun run --bun vite --debug",
    "build": "vue-tsc && vite build",
    "preview": "bun run --bun vite preview",
    "fmt": "prettier --write .",
    "fmt:check": "prettier --check ."
  },
```