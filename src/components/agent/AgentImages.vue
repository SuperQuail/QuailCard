<script setup lang="ts">
import { X } from "@lucide/vue";
import type { AgentImage } from "../../domain/agent";
defineProps<{ images: AgentImage[]; editable?: boolean; disabled?: boolean }>();
const emit = defineEmits<{ remove: [index: number] }>();
</script>

<template>
  <div v-if="images.length" class="mb-2 flex flex-wrap gap-2" aria-label="消息图片">
    <div v-for="(image, index) in images" :key="index" class="relative rounded-lg border border-hairline bg-bg-side p-1">
      <img :src="`data:${image.mimeType};base64,${image.dataBase64}`" :alt="image.name" class="max-h-32 max-w-48 rounded object-contain" />
      <button v-if="editable" class="absolute right-1 top-1 grid size-6 place-items-center rounded-full bg-bg-paper text-ink shadow" :disabled="disabled" :aria-label="`移除图片 ${index + 1}`" @click="emit('remove', index)"><X :size="14" /></button>
    </div>
  </div>
</template>
