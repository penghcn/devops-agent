import type { AgentRequest, AgentResponse, JenkinsCache, StreamEvent } from '../types'

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

export async function callAgentStream(
  request: AgentRequest,
  onEvent: (event: StreamEvent) => void,
): Promise<void> {
  const response = await fetch(`${API_BASE}/agent/stream`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
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
  const response = await fetch(`${API_BASE}/cache`)
  if (!response.ok) {
    throw new Error(`Failed to fetch cache: ${response.status}`)
  }
  return response.json() as Promise<JenkinsCache>
}
