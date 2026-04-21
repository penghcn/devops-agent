import type { AgentRequest, AgentResponse, JenkinsCache } from '../types'

const API_BASE = '/api'

export async function callAgent(request: AgentRequest): Promise<AgentResponse> {
  const response = await fetch(`${API_BASE}/agent`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  })
  if (!response.ok) {
    throw new Error(`HTTP error! status: ${response.status}`)
  }
  return response.json() as Promise<AgentResponse>
}

export async function fetchCache(): Promise<JenkinsCache> {
  const response = await fetch(`${API_BASE}/cache`)
  if (!response.ok) {
    throw new Error(`Failed to fetch cache: ${response.status}`)
  }
  return response.json() as Promise<JenkinsCache>
}
