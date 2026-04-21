export type TaskType = 'Auto' | 'Deploy' | 'Build' | 'Query'

export interface AgentRequest {
  prompt: string
  task_type: TaskType
}

export interface AgentStep {
  action: string
  result: string
  elapsed?: number
}

export interface AgentResponse {
  success: boolean
  output: string
  steps: AgentStep[]
  structured_output?: Record<string, any>
  branch_correction?: string
}

export interface JenkinsCache {
  jobs: { name: string; job_type: string; url: string; branches: string[] }[]
  last_refresh: string
}

export interface ChatMessage {
  id: number
  user: string
  agent: string
  steps: AgentStep[]
  structured_output?: Record<string, any>
  branch_correction?: string
  _elapsed?: number
}
