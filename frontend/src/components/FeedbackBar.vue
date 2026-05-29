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
import { ref, computed } from 'vue'
import type { KnowledgeHit } from '../types'

const props = defineProps<{
  hit: KnowledgeHit
}>()

const submitted = ref(false)

const sourceText = computed(() => {
  const sourceLabel = props.hit.source === 'fingerprint' ? '指纹精确匹配' : '向量语义匹配'
  return `💡 来自知识库 (${sourceLabel}, 置信度 ${(props.hit.confidence * 100).toFixed(0)}%)`
})

async function handleConfirm() {
  if (submitted.value) return
  submitted.value = true
  await sendFeedback(props.hit.entry_id, 'confirm')
}

async function handleDeny() {
  if (submitted.value) return
  submitted.value = true
  await sendFeedback(props.hit.entry_id, 'deny')
}

async function sendFeedback(entryId: number, action: string) {
  try {
    await fetch('/api/knowledge/feedback', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ entry_id: entryId, action }),
    })
  } catch (err) {
    console.error('Feedback error:', err)
  }
}
</script>
