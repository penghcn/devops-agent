import { request } from './client'

const API_BASE = '/api'

export interface DailyStat {
  day: string
  project_name: string
  total_builds: number
  failed_builds: number
  success_rate: number
}

export interface CategoryStat {
  error_category: string
  count: number
  percentage: number
}

export interface TopFailure {
  project_name: string
  total_builds: number
  failed_builds: number
  failure_rate: number
}

export interface KnowledgeTop {
  fingerprint: string
  error_text: string
  hit_count: number
  category: string
}

/**
 * 获取近 N 天每日聚合
 */
export async function getDailyStats(days = 7): Promise<DailyStat[]> {
  const response = await request(`${API_BASE}/stats/daily?days=${days}`)
  if (!response.ok) throw new Error(`Daily stats failed: ${response.status}`)
  return response.json() as Promise<DailyStat[]>
}

/**
 * 获取错误分类占比
 */
export async function getCategoryStats(): Promise<CategoryStat[]> {
  const response = await request(`${API_BASE}/stats/categories`)
  if (!response.ok) throw new Error(`Category stats failed: ${response.status}`)
  return response.json() as Promise<CategoryStat[]>
}

/**
 * 获取失败率 Top 10 项目
 */
export async function getTopFailures(): Promise<TopFailure[]> {
  const response = await request(`${API_BASE}/stats/top-failures`)
  if (!response.ok) throw new Error(`Top failures failed: ${response.status}`)
  return response.json() as Promise<TopFailure[]>
}

/**
 * 获取知识库命中排行
 */
export async function getKnowledgeTop(): Promise<KnowledgeTop[]> {
  const response = await request(`${API_BASE}/stats/knowledge-top`)
  if (!response.ok) throw new Error(`Knowledge top failed: ${response.status}`)
  return response.json() as Promise<KnowledgeTop[]>
}
