<template>
  <div class="space-y-2">
    <!-- 用户消息 -->
    <div class="flex justify-end">
      <div class="max-w-[80%] bg-blue-500 text-white rounded-lg px-4 py-2 text-sm">
        {{ msg.user }}
      </div>
    </div>

    <!-- Agent 回复 -->
    <div class="flex justify-start">
      <div class="max-w-[95%] bg-gray-50 border border-gray-200 rounded-lg px-4 py-3">
        <div v-if="msg._elapsed" class="text-xs text-gray-400 mb-1">
          耗时 {{ formatElapsed(msg._elapsed) }}
        </div>
        <div v-for="(corr, idx) in msg.corrections" :key="idx" class="text-xs text-amber-600 bg-amber-50 border border-amber-200 rounded px-2 py-1 mb-1 flex items-center gap-1">
          <span>⚠️</span>
          <span>{{ corr.kind }} '{{ corr.original }}' 已修正为 '{{ corr.corrected }}'</span>
        </div>
        <StructuredResponse
          v-if="msg.structured_output && Object.keys(msg.structured_output).length > 0"
          :data="msg.structured_output"
        />
        <div v-else class="text-gray-800 text-sm whitespace-pre-wrap">
          {{ msg.agent }}
        </div>
        <FeedbackBar
          :entry-id="msg.knowledge_hit?.entry_id"
          :source-text="feedbackSourceText(msg)"
          :solution="msg.agent"
        />
        <details
          v-if="msg.steps && msg.steps.length > 0"
          :open="!msg._elapsed"
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
</template>

<script setup lang="ts">
import type { ChatMessage } from '../types'
import StructuredResponse from './StructuredResponse.vue'
import FeedbackBar from './FeedbackBar.vue'

defineProps<{
  msg: ChatMessage
}>()

function formatElapsed(seconds: number): string {
  const m = Math.floor(seconds / 60)
  const s = seconds % 60
  if (m > 0) return `${m}分${s.toFixed(2)}秒`
  return `${s.toFixed(2)}秒`
}

function feedbackSourceText(msg: ChatMessage): string {
  if (msg.knowledge_hit) {
    const label = msg.knowledge_hit.source === 'fingerprint' ? '指纹精确匹配' : '向量语义匹配'
    return `💡 知识库 (${label}, ${(msg.knowledge_hit.confidence * 100).toFixed(0)}%)`
  }
  return '🤖 AI 分析结果'
}
</script>
