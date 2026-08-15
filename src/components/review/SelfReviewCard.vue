<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import type { ReviewCard } from "../../domain/types";
import RatingRow from "./RatingRow.vue";

/**
 * 自评问答卡：先回想再揭示参考答案，随后自评。
 * 本地揭示状态随卡片 :key 重建自动重置。
 */
defineProps<{
  card: ReviewCard;
  busy: boolean;
}>();

const emit = defineEmits<{
  rate: [rating: "again" | "hard" | "good"];
}>();

const revealed = ref(false);

/** 显示参考答案。 */
function reveal(): void {
  revealed.value = true;
}

/** 自评卡键盘：空格揭示答案，1/2/3 自评。 */
function handleKeydown(event: KeyboardEvent): void {
  const target = event.target as HTMLElement | null;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    return;
  }
  if (event.key === " ") {
    event.preventDefault();
    if (!revealed.value) {
      reveal();
    }
    return;
  }
  if (["1", "2", "3"].includes(event.key) && revealed.value) {
    emit("rate", event.key === "1" ? "again" : event.key === "2" ? "hard" : "good");
  }
}

onMounted(() => window.addEventListener("keydown", handleKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", handleKeydown));
</script>

<template>
  <div class="text-center">
    <p class="text-[11px] tracking-wide text-ink-3 uppercase">问答</p>
    <h2 class="note-title mt-4 !text-[22px]">{{ card.front }}</h2>
  </div>
  <div v-if="revealed" class="mt-8 rounded-lg bg-bg-side p-5">
    <p class="text-[10px] font-medium tracking-wide text-accent-strong">参考答案</p>
    <p class="mt-2 font-serif text-[15px] leading-7">{{ card.back }}</p>
    <p v-if="card.detail" class="mt-2 text-[10px] text-ink-3">{{ card.detail }}</p>
  </div>
  <div v-else class="mt-8 flex justify-center">
    <button type="button" class="primary-btn !h-10 !px-6" @click="reveal">显示答案 <kbd class="kbd ml-1 !border-transparent !bg-white/20 !text-white">空格</kbd></button>
  </div>

  <RatingRow v-if="revealed" :busy="busy" @rate="emit('rate', $event)" />
</template>
