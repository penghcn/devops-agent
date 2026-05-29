<template>
  <div class="feedback-bar mt-2 pt-2 border-t border-gray-200">
    <div class="flex items-center justify-between text-xs">
      <span class="text-gray-500">{{ sourceText }}</span>
      <div class="flex gap-2">
        <button
          @click="handleConfirm"
          :disabled="submitted"
          class="px-2 py-1 rounded hover:bg-green-100 disabled:opacity-50 transition-colors"
        >
          {{ submitted ? '✓ 已反馈' : '👍 有用' }}
        </button>
        <button
          @click="handleDeny"
          :disabled="submitted"
          class="px-2 py-1 rounded hover:bg-red-100 disabled:opacity-50 transition-colors"
        >
          {{ submitted ? '✓ 已反馈' : '👎 无用' }}
        </button>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'
import { request } from '../api/client'

const props = defineProps<{
  /** 知识库条目 ID（可选，LLM 方案时无此值） */
  entryId?: number
  /** 来源说明文本 */
  sourceText: string
  /** AI 生成的解决方案文本（仅当无 entryId 时使用） */
  solution?: string
}>()

const submitted = ref(false)

async function handleConfirm() {
  if (submitted.value) return
  submitted.value = true
  if (props.entryId) {
    // Flow A: 更新已有条目置信度
    await request('/api/knowledge/feedback', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ entry_id: props.entryId, action: 'confirm' }),
    })
  } else {
    // Flow B: 写入新条目到知识库
    await request('/api/knowledge/learn', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ solution: props.solution }),
    })
  }
}

async function handleDeny() {
  if (submitted.value) return
  submitted.value = true
  if (props.entryId) {
    await request('/api/knowledge/feedback', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ entry_id: props.entryId, action: 'deny' }),
    })
  }
}
</script>
