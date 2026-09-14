<script setup lang="ts">
import { LogIn, RotateCw, TriangleAlert, X } from "@lucide/vue";
import type { VideoLoginStatus } from "../../domain/video";

defineProps<{ login: VideoLoginStatus | null }>();
const emit = defineEmits<{ close: []; retry: [] }>();

/** 失败或过期时提供重试入口而不是静默关闭。 */
function broken(state: string): boolean {
  return ["expired", "timeout", "failed"].includes(state);
}
</script>

<template>
  <div class="fixed inset-0 z-80 grid place-items-center bg-ink/30 px-4" role="dialog" aria-label="扫码登录 B 站">
    <section class="w-full max-w-[340px] space-y-3 rounded-2xl border border-hairline bg-bg-paper p-5 text-center">
      <header class="flex items-center justify-between">
        <p class="flex items-center gap-2 text-[13px] font-medium"><LogIn :size="15" :stroke-width="1.8" />扫码登录 B 站</p>
        <button class="icon-btn" title="关闭" @click="emit('close')"><X :size="16" /></button>
      </header>

      <img v-if="login?.image" :src="login.image" alt="B 站登录二维码" class="mx-auto size-[220px] rounded-xl bg-white p-2" />
      <p v-else class="grid h-[220px] place-items-center text-[12px] text-ink-2">
        <span v-if="login && broken(login.state)" class="flex items-center gap-2"><TriangleAlert :size="14" />二维码不可用，请重新获取</span>
        <span v-else class="flex items-center gap-2"><RotateCw :size="14" class="animate-spin" />正在获取二维码…</span>
      </p>

      <p class="text-[12px] text-ink-2">{{ login?.message ?? "请使用 B 站客户端扫码" }}</p>
      <p v-if="login && broken(login.state)" class="flex items-center justify-center gap-1 text-[12px] text-ink-2">
        <TriangleAlert :size="13" />可重新获取二维码
      </p>
      <button v-if="login && broken(login.state)" class="primary-btn mx-auto" @click="emit('retry')">
        <RotateCw :size="14" />重新获取
      </button>
      <p class="text-[11px] text-ink-3">Cookie 只保存在本机加密保险库，不会写入日志或上传。</p>
    </section>
  </div>
</template>
