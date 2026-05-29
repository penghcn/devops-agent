/// <reference types="vite/client" />

declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<{}, {}, any>
  export default component
}

interface ImportMetaEnv {
  readonly VITE_GITLAB_URL: string
  readonly VITE_GITLAB_CLIENT_ID: string
  readonly VITE_API_BASE_URL?: string
}
