import { request } from './client'

const API_BASE = '/api'

export interface KnowledgeEntry {
  id: number
  fingerprint: string
  error_text: string
  solution: string
  category: string
  confidence: number
  hit_count: number
  confirm_count: number
  deny_count: number
  source_build?: string
  created_at: string
}

export interface KnowledgeSearchResult {
  entry_id: number
  solution: string
  confidence: number
  category: string
  source: 'exact_fingerprint' | 'embedding_similar'
}

export interface FeedbackRequest {
  entry_id: number
  action: 'confirm' | 'deny'
}

/**
 * 搜索知识库
 */
export async function searchKnowledge(buildLog: string): Promise<KnowledgeSearchResult | null> {
  const response = await request(`${API_BASE}/knowledge/search`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ build_log: buildLog }),
  })
  if (response.status === 404) return null
  if (!response.ok) throw new Error(`Knowledge search failed: ${response.status}`)
  return response.json() as Promise<KnowledgeSearchResult>
}

/**
 * 提交反馈
 */
export async function submitFeedback(req: FeedbackRequest): Promise<void> {
  const response = await request(`${API_BASE}/knowledge/feedback`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(req),
  })
  if (!response.ok) throw new Error(`Feedback failed: ${response.status}`)
}

/**
 * 获取知识库列表
 */
export async function listKnowledge(page = 1, limit = 20): Promise<KnowledgeEntry[]> {
  const response = await request(
    `${API_BASE}/knowledge/entries?page=${page}&limit=${limit}`
  )
  if (!response.ok) throw new Error(`List knowledge failed: ${response.status}`)
  return response.json() as Promise<KnowledgeEntry[]>
}
