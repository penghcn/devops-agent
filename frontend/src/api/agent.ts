import type { AgentRequest, AgentResponse, JenkinsCache, StreamEvent } from '../types'
import { request } from './client'

const API_BASE = '/api'

export async function callAgent(requestBody: AgentRequest): Promise<AgentResponse> {
  const response = await request(`${API_BASE}/agent`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(requestBody),
  })
  if (!response.ok) {
    throw new Error(`HTTP error! status: ${response.status}`)
  }
  return response.json() as Promise<AgentResponse>
}

export async function callAgentStream(
  requestBody: AgentRequest,
  onEvent: (event: StreamEvent) => void,
): Promise<void> {
  const response = await request(`${API_BASE}/agent/stream`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(requestBody),
  })
  if (!response.ok) {
    throw new Error(`HTTP error! status: ${response.status}`)
  }

  const reader = response.body?.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  if (!reader) throw new Error('Response body is null')

  try {
    while (true) {
      const { done, value } = await reader.read()
      if (done) break

      buffer += decoder.decode(value, { stream: true })
      const lines = buffer.split('\n\n')
      buffer = lines.pop() || ''

      for (const line of lines) {
        const dataLine = line.split('\n').find(l => l.startsWith('data:'))
        if (dataLine) {
          const json = dataLine.slice(5).trim()
          try {
            const event: StreamEvent = JSON.parse(json)
            onEvent(event)
          } catch {
            // ignore malformed SSE
          }
        }
      }
    }
  } finally {
    reader.releaseLock()
  }
}

export async function fetchCache(): Promise<JenkinsCache> {
  const response = await request(`${API_BASE}/cache`)
  if (!response.ok) {
    throw new Error(`Failed to fetch cache: ${response.status}`)
  }
  return response.json() as Promise<JenkinsCache>
}
