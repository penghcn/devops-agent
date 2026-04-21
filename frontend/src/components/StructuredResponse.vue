<script setup lang="ts">
interface Props {
  data: Record<string, any>
}

const props = defineProps<Props>()

const isDeploySuccess = props.data.deploy_status === 'success'
const isDeployFailed = props.data.deploy_status === 'failed'

function statusColor(status: string) {
  if (status === 'success') return 'bg-green-100 text-green-800'
  if (status === 'failed') return 'bg-red-100 text-red-800'
  return 'bg-gray-100 text-gray-800'
}

function statusLabel(status: string) {
  if (status === 'success') return '部署成功'
  if (status === 'failed') return '部署失败'
  if (status === 'failed' || status === 'FAILURE') return '构建失败'
  return status
}
</script>

<template>
  <div class="mt-2 p-3 rounded-lg border" :class="isDeploySuccess ? 'border-green-200 bg-green-50' : isDeployFailed ? 'border-red-200 bg-red-50' : 'border-blue-200 bg-blue-50'">
    <!-- 部署成功 -->
    <div v-if="isDeploySuccess">
      <div class="flex items-center gap-2 mb-2">
        <span class="text-lg">✅</span>
        <span class="font-medium text-green-800">部署成功</span>
      </div>
      <div v-if="data.servers && data.servers.length" class="mb-2">
        <div class="text-xs text-green-600 mb-1">目标服务器</div>
        <div class="flex flex-wrap gap-1">
          <span
            v-for="server in data.servers"
            :key="server"
            class="inline-block px-2 py-0.5 text-xs bg-green-200 text-green-800 rounded"
          >
            {{ server }}
          </span>
        </div>
      </div>
      <div v-if="data.summary" class="text-sm text-green-700">
        {{ data.summary }}
      </div>
    </div>

    <!-- 部署失败 -->
    <div v-else-if="isDeployFailed">
      <div class="flex items-center gap-2 mb-2">
        <span class="text-lg">❌</span>
        <span class="font-medium text-red-800">部署失败</span>
      </div>
      <div v-if="data.error" class="mb-2 text-sm text-red-700">
        <div class="text-xs text-red-600 mb-1">错误原因</div>
        {{ data.error }}
      </div>
      <div v-if="data.suggestion" class="text-sm text-red-700">
        <div class="text-xs text-red-600 mb-1">建议</div>
        {{ data.suggestion }}
      </div>
    </div>

    <!-- 构建分析 -->
    <div v-else>
      <div class="flex items-center gap-2 mb-2">
        <span class="text-lg">📊</span>
        <span
          class="font-medium"
          :class="data.build_status === 'SUCCESS' ? 'text-green-700' : 'text-orange-700'"
        >
          {{ statusLabel(data.build_status || '分析中') }}
        </span>
      </div>
      <div v-if="data.error" class="mb-2 text-sm">
        <div class="text-xs text-gray-600 mb-1">错误分析</div>
        {{ data.error }}
      </div>
      <div v-if="data.suggestion" class="text-sm">
        <div class="text-xs text-gray-600 mb-1">建议</div>
        {{ data.suggestion }}
      </div>
    </div>
  </div>
</template>
