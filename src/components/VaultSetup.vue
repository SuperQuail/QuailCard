<script setup lang="ts">
import { Clock3, FolderOpen } from "@lucide/vue";
import { ref } from "vue";
import { isTauri } from "../api/backend";

defineProps<{
  recents: string[];
}>();

const emit = defineEmits<{
  "open-vault": [path: string];
}>();

const vaultPath = ref("");
const errorMessage = ref("");
const busy = ref(false);

/** 使用系统原生对话框选择文件夹。 */
async function pickFolder(): Promise<void> {
  if (!isTauri()) {
    // 浏览器演示环境回退到手工输入。
    return;
  }
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({ directory: true, title: "选择 Vault 文件夹" });
    if (typeof selected === "string" && selected) {
      vaultPath.value = selected;
    }
  } catch {
    // 对话框打开失败时保留手工输入。
  }
}

/** 提交 Vault 路径。 */
async function submit(path?: string): Promise<void> {
  const target = (path ?? vaultPath.value).trim();
  if (!target) {
    errorMessage.value = "请输入或选择文件夹路径";
    return;
  }
  if (busy.value) {
    return;
  }
  busy.value = true;
  errorMessage.value = "";
  try {
    emit("open-vault", target);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="flex min-h-screen items-center justify-center bg-bg px-6">
    <div class="flex w-full max-w-[400px] flex-col items-center text-center">
      <span class="grid size-12 place-items-center rounded-xl bg-accent text-[20px] font-bold text-white">Q</span>
      <h1 class="mt-5 text-[22px] font-semibold tracking-tight">QuailCard</h1>
      <p class="mt-1 text-[12px] text-ink-3">本地 Markdown 笔记 + 记忆卡片</p>

      <!-- 历史 Vault -->
      <div v-if="recents.length > 0" class="mt-8 w-full text-left">
        <p class="mb-1.5 text-[11px] font-medium text-ink-2">最近打开</p>
        <div class="space-y-1">
          <button
            v-for="path in recents"
            :key="path"
            type="button"
            class="flex w-full items-center gap-2 rounded-lg border border-hairline bg-bg-paper px-3 py-2 text-left transition hover:border-accent"
            :disabled="busy"
            @click="submit(path)"
          >
            <Clock3 :size="13" class="shrink-0 text-ink-3" />
            <span class="min-w-0 truncate text-[12px] text-ink-2">{{ path }}</span>
          </button>
        </div>
      </div>

      <!-- 新建 / 打开其他文件夹 -->
      <div class="mt-6 w-full text-left">
        <p class="mb-1.5 text-[11px] font-medium text-ink-2">其他文件夹</p>
        <div class="flex items-center gap-2 rounded-lg border border-hairline bg-bg-paper px-3 py-2">
          <FolderOpen :size="14" class="shrink-0 text-ink-3" />
          <input v-model="vaultPath" class="min-w-0 flex-1 bg-transparent text-[12px] outline-none" placeholder="输入路径，或点右侧浏览" @keyup.enter="submit()" />
          <button type="button" class="ghost-btn shrink-0 border border-hairline" @click="pickFolder">浏览…</button>
        </div>
        <p v-if="errorMessage" class="mt-1.5 text-[11px] text-danger">{{ errorMessage }}</p>
      </div>

      <button type="button" class="primary-btn mt-6 w-full !h-10" :disabled="!vaultPath.trim() || busy" @click="submit()">
        {{ busy ? "正在打开…" : "打开这个文件夹" }}
      </button>
    </div>
  </div>
</template>
