<!-- frontend/src/App.vue -->
<template>
  <div class="min-h-screen bg-gray-100 p-4">
    <div class="max-w-2xl mx-auto bg-white rounded-lg shadow">
      <div class="p-4 border-b">
        <h1 class="text-xl font-bold text-gray-800">DevOps Agent</h1>
      </div>

      <!-- 聊天记录 -->
      <div ref="chatContainer" class="h-96 overflow-y-auto p-4 space-y-4">
        <div v-for="msg in messages" :key="msg.id" class="space-y-1">
          <div class="text-right text-blue-600 font-medium">用户: {{ msg.user }}</div>
          <div class="text-left text-green-700">
            <div>Agent: {{ msg.agent }}</div>
            <details v-if="msg.steps && msg.steps.length > 0" class="text-xs text-gray-500 mt-1">
              <summary>思考过程</summary>
              <ul class="list-disc pl-4 mt-1">
                <li v-for="step in msg.steps" :key="step.action">{{ step.action }}: {{ step.result }}</li>
              </ul>
            </details>
          </div>
        </div>
        <div v-if="loading" class="text-gray-400 italic">Agent 思考中...</div>
      </div>

      <!-- 输入框 -->
      <div class="border-t p-4">
        <div class="flex gap-2">
          <input
            v-model="input"
            @keyup.enter="handleSend"
            class="flex-1 border border-gray-300 rounded-lg px-4 py-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
            placeholder="输入指令，如：部署 order-service 到 staging"
          />
          <button
            @click="handleSend"
            :disabled="loading || !input.trim()"
            class="bg-blue-500 text-white px-6 py-2 rounded-lg hover:bg-blue-600 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {{ loading ? '处理中...' : '发送' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, nextTick } from 'vue'
import { callAgent } from './api/agent'
import type { ChatMessage } from './types'

const input = ref('')
const messages = ref<ChatMessage[]>([])
const loading = ref(false)
const chatContainer = ref<HTMLDivElement>()

async function handleSend() {
  if (!input.value.trim() || loading.value) return

  const userMsg = input.value.trim()
  loading.value = true

  try {
    const data = await callAgent({ prompt: userMsg, task_type: 'Auto' })

    messages.value.push({
      id: Date.now(),
      user: userMsg,
      agent: data.output,
      steps: data.steps || []
    })

    // 滚动到底部
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
      steps: []
    })
  } finally {
    loading.value = false
    input.value = ''
  }
}
</script>
