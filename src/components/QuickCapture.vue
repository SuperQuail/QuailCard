<script setup lang="ts">
import { PenLine, X } from "@lucide/vue";
import { ref } from "vue";

const props = defineProps<{ folders: string[] }>();

const emit = defineEmits<{
  close: [];
  create: [title: string, folder: string, body: string];
}>();

const title = ref("");
const body = ref("");
const folder = ref("收件箱");

/** 提交快速捕获。 */
function submit(): void {
  const trimmedTitle = title.value.trim();
  const trimmedBody = body.value.trim();
  if (!trimmedTitle && !trimmedBody) {
    return;
  }
  // 标题为空时用正文首行作为标题
  const finalTitle = trimmedTitle || trimmedBody.split("\n")[0].slice(0, 40);
  const finalBody = trimmedTitle ? trimmedBody : trimmedBody;
  emit("create", finalTitle, folder.value, finalBody);
}
</script>

<template>
  <div class="overlay-backdrop flex items-start justify-center pt-32" @click.self="emit('close')">
    <div class="modal-panel w-full max-w-[460px] p-5">
      <header class="mb-4 flex items-center justify-between">
        <h2 class="flex items-center gap-2 text-[14px] font-semibold">
          <PenLine :size="15" class="text-accent-strong" />快速捕获
        </h2>
        <button type="button" class="icon-btn" aria-label="关闭" @click="emit('close')">
          <X :size="15" />
        </button>
      </header>

      <label class="mb-3 block">
        <span class="mb-1 block text-[11px] font-medium text-ink-2">标题</span>
        <input v-model="title" class="field-input" placeholder="留空时取正文第一行" @keyup.enter="submit" />
      </label>

      <label class="mb-3 block">
        <span class="mb-1 block text-[11px] font-medium text-ink-2">内容</span>
        <textarea v-model="body" class="field-textarea !min-h-28 font-serif !text-[14px]" placeholder="随手写点什么，之后再来整理…" />
      </label>

      <label class="mb-4 block">
        <span class="mb-1 block text-[11px] font-medium text-ink-2">保存到</span>
        <select v-model="folder" class="field-input">
          <option v-for="name in props.folders" :key="name" :value="name">{{ name }}</option>
        </select>
      </label>

      <footer class="flex justify-end gap-2">
        <button type="button" class="ghost-btn" @click="emit('close')">取消</button>
        <button type="button" class="primary-btn" :disabled="!title.trim() && !body.trim()" @click="submit">
          保存为新笔记
        </button>
      </footer>
    </div>
  </div>
</template>
