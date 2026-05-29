import { request, setToken } from './client'

const API_BASE = '/api'

export interface LoginResponse {
  access_token: string
  refresh_token?: string
  expires_in: number
}

export interface UserInfo {
  username: string
  gitlab_id: string
  avatar_url?: string
  role: string
}

/**
 * 跳转 GitLab OAuth
 */
export function gitlabLogin(): void {
  window.location.href = `${API_BASE}/auth/gitlab/login`
}

/**
 * 处理 OAuth callback 参数中的 token
 */
export async function handleCallback(code: string): Promise<LoginResponse> {
  const response = await request(`${API_BASE}/auth/callback?code=${code}`, {
    method: 'POST',
  })
  if (!response.ok) {
    throw new Error(`Auth callback failed: ${response.status}`)
  }
  const data: LoginResponse = await response.json()
  setToken(data.access_token)
  return data
}

/**
 * 刷新 Token
 */
export async function refreshToken(refreshToken: string): Promise<LoginResponse> {
  const response = await request(`${API_BASE}/auth/refresh`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ refresh_token: refreshToken }),
  })
  if (!response.ok) {
    throw new Error(`Token refresh failed: ${response.status}`)
  }
  const data: LoginResponse = await response.json()
  setToken(data.access_token)
  return data
}

/**
 * 获取当前用户信息
 */
export async function getCurrentUser(): Promise<UserInfo> {
  const response = await request(`${API_BASE}/auth/me`)
  if (!response.ok) {
    throw new Error(`Get user info failed: ${response.status}`)
  }
  return response.json() as Promise<UserInfo>
}

/**
 * 登出
 */
export async function logout(): Promise<void> {
  await request(`${API_BASE}/auth/logout`, { method: 'POST' })
}
