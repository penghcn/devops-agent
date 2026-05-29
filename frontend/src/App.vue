<template>
  <div class="min-h-screen bg-gray-100 p-4">
    <!-- 顶部导航 -->
    <nav class="max-w-6xl mx-auto mb-4 flex gap-2">
      <router-link
        v-for="item in navItems"
        :key="item.path"
        :to="item.path"
        class="px-4 py-2 rounded-lg text-sm font-medium transition-colors"
        :class="route.path === item.path
          ? 'bg-blue-600 text-white'
          : 'bg-white text-gray-700 hover:bg-gray-50'"
      >
        {{ item.label }}
      </router-link>
    </nav>

    <!-- 路由视图 -->
    <router-view />
  </div>
</template>

<script setup lang="ts">
import { useRoute } from 'vue-router'

const route = useRoute()

const navItems = [
  { path: '/chat', label: '对话' },
  { path: '/dashboard', label: '看板' },
]
</script>
