<script setup lang="ts">
/** 自评分数行：忘记/困难/记得，动作上抛。 */
defineProps<{
  busy: boolean;
}>();

const emit = defineEmits<{
  rate: [rating: "again" | "hard" | "good"];
}>();
</script>

<template>
  <div class="mt-7 grid grid-cols-3 gap-2">
    <button type="button" class="flex h-11 items-center justify-center gap-2 rounded-lg border border-hairline bg-bg-paper text-[13px] font-medium transition hover:border-danger hover:text-danger" :disabled="busy" @click="emit('rate', 'again')">
      忘记 <kbd class="kbd">1</kbd>
    </button>
    <button type="button" class="flex h-11 items-center justify-center gap-2 rounded-lg border border-hairline bg-bg-paper text-[13px] font-medium transition hover:border-warning hover:text-warning" :disabled="busy" @click="emit('rate', 'hard')">
      困难 <kbd class="kbd">2</kbd>
    </button>
    <button type="button" class="flex h-11 items-center justify-center gap-2 rounded-lg border border-hairline bg-bg-paper text-[13px] font-medium transition hover:border-success hover:text-success" :disabled="busy" @click="emit('rate', 'good')">
      记得 <kbd class="kbd">3</kbd>
    </button>
  </div>
</template>
