<!-- frontend/src/App.vue -->
<template>
  <div class="min-h-screen bg-gray-100 p-4">
    <div class="max-w-2xl mx-auto bg-white rounded-lg shadow">
      <div class="p-4 border-b bg-gradient-to-r from-blue-600 to-blue-700 rounded-t-lg">
        <div class="flex items-center justify-between">
          <h1 class="text-xl font-bold text-white">Jenkins DevOps Agent</h1>
          <div class="text-xs text-blue-200">
            <span v-if="cache">共 {{ cache.jobs.length }} 个项目 · {{ formatLocalTime(cache.last_refresh) }}</span>
            <span v-else class="animate-pulse">加载中...</span>
          </div>
        </div>
      </div>

      <!-- 项目/分支选择器 -->
      <div class="p-3 border-b bg-gray-50 flex gap-2 items-center">
        <select
          v-model="selectedJob"
          class="flex-1 border border-gray-300 rounded px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
        >
          <option value="">选择项目</option>
          <optgroup v-for="job in jobs" :key="job.name" :label="job.job_type === 'pipeline_multibranch' ? '🔀 Pipeline' : '📦 Job'">
            <option :value="job.name">{{ job.name }}</option>
          </optgroup>
        </select>
        <select
          v-if="selectedJob"
          v-model="selectedBranch"
          :disabled="branches.length === 0"
          class="flex-1 border border-gray-300 rounded px-3 py-1.5 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 disabled:opacity-50"
        >
          <option value="">默认分支</option>
          <option v-for="b in branches" :key="b" :value="b">{{ b }}</option>
        </select>
      </div>

      <!-- 聊天记录 -->
      <div ref="chatContainer" class="h-96 overflow-y-auto p-4 space-y-4">
        <div v-for="msg in messages" :key="msg.id" class="space-y-2">
          <!-- 用户消息 -->
          <div class="flex justify-end">
            <div class="max-w-[80%] bg-blue-500 text-white rounded-lg px-4 py-2 text-sm">
              {{ msg.user }}
            </div>
          </div>

          <!-- Agent 回复 -->
          <div class="flex justify-start">
            <div class="max-w-[85%] bg-gray-50 border border-gray-200 rounded-lg px-4 py-3">
              <div v-if="msg._elapsed" class="text-xs text-gray-400 mb-1">
                耗时 {{ formatElapsed(msg._elapsed) }}
              </div>
              <div v-if="msg.branch_correction" class="text-xs text-amber-600 bg-amber-50 border border-amber-200 rounded px-2 py-1 mb-2 flex items-center gap-1">
                <span>⚠️</span>
                <span>{{ msg.branch_correction }}</span>
              </div>
              <StructuredResponse
                v-if="msg.structured_output && Object.keys(msg.structured_output).length > 0"
                :data="msg.structured_output"
              />
              <div v-else class="text-gray-800 text-sm whitespace-pre-wrap">
                {{ msg.agent }}
              </div>
              <details
                v-if="msg.steps && msg.steps.length > 0"
                class="text-xs text-gray-500 mt-2 border-t pt-2"
              >
                <summary class="cursor-pointer hover:text-gray-700">执行步骤</summary>
                <ul class="list-disc pl-4 mt-1 space-y-1">
                  <li v-for="step in msg.steps" :key="step.action" class="text-gray-600">
                    <span class="font-medium">{{ step.action }}:</span>
                    <span class="ml-1">{{ step.result }}</span>
                    <span v-if="step.elapsed" class="ml-1 text-gray-400">({{ formatElapsed(step.elapsed as number) }})</span>
                  </li>
                </ul>
              </details>
            </div>
          </div>
        </div>
        <div v-if="loading" class="flex items-center gap-2 text-gray-400 text-sm">
          <svg class="animate-spin h-4 w-4" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path>
          </svg>
          Agent 处理中... {{ formatElapsed(loadingElapsed) }}
        </div>
      </div>

      <!-- 输入框 -->
      <div class="border-t p-4">
        <div class="flex gap-2">
          <input
            v-model="input"
            @keyup.enter="handleSend"
            class="flex-1 border border-gray-300 rounded-lg px-4 py-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
            placeholder="输入指令，如：部署 ds-pkg 到 staging"
          />
          <button
            @click="handleSend"
            :disabled="loading || !input.trim()"
            class="bg-blue-500 text-white px-6 py-2 rounded-lg hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            {{ loading ? '处理中...' : '发送' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, nextTick, computed } from 'vue'
import { callAgent, fetchCache } from './api/agent'
import type { ChatMessage, JenkinsCache } from './types'
import StructuredResponse from './components/StructuredResponse.vue'

const input = ref('')
const messages = ref<ChatMessage[]>([])
const loading = ref(false)
const chatContainer = ref<HTMLDivElement>()
const cache = ref<JenkinsCache | null>(null)
const selectedJob = ref('')
const selectedBranch = ref('')
const loadingElapsed = ref(0)

let elapsedTimer: ReturnType<typeof setInterval> | null = null

const jobs = computed(() => cache.value?.jobs || [])
const branches = computed(() => {
  if (!selectedJob.value) return []
  return cache.value?.jobs.find(j => j.name === selectedJob.value)?.branches || []
})

// 启动时加载缓存
fetchCache().then(c => { cache.value = c }).catch(() => {})

// 格式化本地时间（RFC3339 → 本地可读格式）
function formatLocalTime(rfc: string): string {
  if (!rfc) return ''
  const d = new Date(rfc)
  if (isNaN(d.getTime())) return rfc
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

// 格式化耗时
function formatElapsed(seconds: number): string {
  const m = Math.floor(seconds / 60)
  const s = seconds % 60
  if (m > 0) return `${m}分${s}秒`
  return `${s}秒`
}

// 每秒刷新耗时
function startElapsedTimer() {
  loadingElapsed.value = 0
  if (elapsedTimer) clearInterval(elapsedTimer)
  elapsedTimer = setInterval(() => {
    loadingElapsed.value++
  }, 1000)
}

function stopElapsedTimer() {
  if (elapsedTimer) {
    clearInterval(elapsedTimer)
    elapsedTimer = null
  }
}

async function handleSend() {
  if (!input.value.trim() || loading.value) return

  const userMsg = input.value.trim()
  loading.value = true
  startElapsedTimer()

  const startTime = Date.now()

  try {
    const data = await callAgent({
      prompt: userMsg,
      task_type: 'Auto',
    })

    const elapsed = Math.floor((Date.now() - startTime) / 1000)
    stopElapsedTimer()

    messages.value.push({
      id: Date.now(),
      user: userMsg,
      agent: data.output,
      steps: data.steps || [],
      structured_output: data.structured_output,
      branch_correction: data.branch_correction,
      _elapsed: elapsed,
    })

    await nextTick()
    if (chatContainer.value) {
      chatContainer.value.scrollTop = chatContainer.value.scrollHeight
    }
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : '未知错误'
    messages.value.push({
      id: Date.now(),
      user: userMsg,
      agent: `错误: ${message}`,
      steps: [],
    })
    stopElapsedTimer()
  } finally {
    loading.value = false
    input.value = ''
  }
}
</script>
