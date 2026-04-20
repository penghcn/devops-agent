export type TaskType = 'Auto' | 'Deploy' | 'Build' | 'Query'

export interface AgentRequest {
  prompt: string
  task_type: TaskType
}

export interface AgentStep {
  action: string
  result: string
}

export interface AgentResponse {
  success: boolean
  output: string
  steps: AgentStep[]
}

export interface ChatMessage {
  id: number
  user: string
  agent: string
  steps: AgentStep[]
}
