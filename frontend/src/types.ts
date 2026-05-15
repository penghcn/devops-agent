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

export interface Correction {
  kind: string
  original: string
  corrected: string
}

export interface AgentResponse {
  success: boolean
  output: string
  steps: AgentStep[]
  structured_output?: Record<string, any>
  corrections?: Correction[]
}

export type StreamEventType =
  | 'StepStart'
  | 'StepDone'
  | 'BranchCorrection'
  | 'Complete'

export interface StreamEvent {
  type: StreamEventType
  step_index?: number
  action?: string
  description?: string
  result?: string
  elapsed?: number
  message?: string
  success?: boolean
  output?: string
  steps?: AgentStep[]
  structured_output?: Record<string, any>
  corrections?: Correction[]
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
  corrections?: Correction[]
  _elapsed?: number
}
