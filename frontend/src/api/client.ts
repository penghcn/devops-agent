/**
 * HTTP 请求客户端，自动携带 JWT token（如果存在）。
 *
 * 认证策略：可选认证
 * - localStorage 中有 token → 自动注入 Authorization header
 * - 没有 token → 直接发送请求（兼容内部部署）
 */

const TOKEN_KEY = 'devops_auth_token'

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY)
}

export function setToken(token: string): void {
  localStorage.setItem(TOKEN_KEY, token)
}

export function clearToken(): void {
  localStorage.removeItem(TOKEN_KEY)
}

/**
 * 构建带认证 header 的 fetch options
 */
function authHeaders(extra: Record<string, string> = {}): Record<string, string> {
  const headers: Record<string, string> = { ...extra }
  const token = getToken()
  if (token) {
    headers['Authorization'] = `Bearer ${token}`
  }
  return headers
}

/**
 * 封装 fetch，自动注入认证 token
 */
export async function request(url: string, options: RequestInit = {}): Promise<Response> {
  const headers = {
    ...authHeaders(options.headers as Record<string, string> || {}),
  }

  const response = await fetch(url, { ...options, headers })

  // 401 → 清除 token 并跳转登录
  if (response.status === 401) {
    clearToken()
    window.location.href = '/login'
  }

  return response
}
