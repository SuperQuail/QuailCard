<!-- 全局轻提示：只负责展示与进出场动画，消息的生成与清除由壳层决定。 -->
<script setup lang="ts">
defineProps<{ message: string }>();
</script>

<template>
  <Transition name="toast">
    <div
      v-if="message"
      class="fixed bottom-9 left-1/2 z-90 -translate-x-1/2 rounded-lg bg-ink px-3.5 py-2 text-[12px] font-medium text-bg shadow-lg"
    >
      {{ message }}
    </div>
  </Transition>
</template>

<style scoped>
.toast-enter-active,
.toast-leave-active {
  transition: opacity 150ms, transform 150ms;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translate(-50%, 6px);
}
</style>
