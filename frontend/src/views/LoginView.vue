<template>
  <div class="flex items-center justify-center min-h-[60vh]">
    <div class="bg-white rounded-lg shadow p-8 w-full max-w-md">
      <h2 class="text-2xl font-bold text-center mb-6">登录</h2>
      <button
        class="w-full bg-blue-600 text-white py-2 rounded-lg hover:bg-blue-700 transition-colors flex items-center justify-center gap-2"
        @click="handleGitLabLogin"
      >
        <svg class="w-5 h-5" viewBox="0 0 24 24" fill="currentColor">
          <path d="M22.56 12.25c0-.75-.08-1.49-.26-2.21l.26-.04.01-.03A12.01 12.01 0 0012 2.05a12.01 12.01 0 00-10.57 6.06l.01.03.26.04c-.18.72-.26 1.46-.26 2.21 0 .92.15 1.8.42 2.63l-.42.07v.04A12.01 12.01 0 0012 21.95a12.01 12.01 0 0010.57-6.06l-.01-.04-.42-.07c.27-.83.42-1.71.42-2.63zM12 15.5a3.5 3.5 0 110-7 3.5 3.5 0 010 7z" />
        </svg>
        使用 GitLab 登录
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
function handleGitLabLogin() {
  // TODO: Phase 1 - 跳转 GitLab OAuth
  const gitlabUrl = import.meta.env.VITE_GITLAB_URL || 'https://gitlab.com'
  const clientId = import.meta.env.VITE_GITLAB_CLIENT_ID
  const redirectUri = `${window.location.origin}/login/callback`
  window.location.href = `${gitlabUrl}/oauth/authorize?client_id=${clientId}&redirect_uri=${encodeURIComponent(redirectUri)}&response_type=code`
}
</script>
