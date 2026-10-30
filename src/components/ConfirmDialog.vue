<!----------------------------------------------------------------
 通用危险操作确认框：只渲染标题、正文与两个选择按钮，
 不含业务状态；是否危险、是否展示由调用方决定。
---------------------------------------------------------------->
<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, useId, watch } from "vue";

const props = defineProps<{
  /** 是否展示；为 false 时不渲染任何内容。 */
  open: boolean;
  /** 对话框标题，同时作为可访问名称。 */
  title: string;
  /** 正文：说明本次操作的后果，不含交互逻辑。 */
  body: string;
  /** 确认按钮文案，语义由调用方决定（例如「永久删除」）。 */
  confirmLabel: string;
  /** 确认按钮是否使用危险色：删除类操作为 true。 */
  danger?: boolean;
}>();

const emit = defineEmits<{ confirm: []; cancel: [] }>();

/** 标题与正文的唯一 id，供 aria-labelledby / aria-describedby 引用。 */
const titleId = useId();
const bodyId = `${titleId}-body`;

/** 取消按钮：默认焦点落在它上面，避免回车直接触发危险操作。 */
const cancelButton = ref<HTMLButtonElement | null>(null);

/** 打开后把焦点移到取消按钮；关闭时不抢占底层界面焦点。 */
watch(() => props.open, (open) => {
  if (open) {
    void nextTick(() => cancelButton.value?.focus());
  }
}, { immediate: true });

/**
 * Esc 取消；回车按焦点语义解释，避免未读内容的用户误按回车永久删除。
 *
 * 契约：
 * - 焦点在「取消」按钮（默认焦点）→ 回车取消；
 * - 焦点在确认按钮 → 回车确认（等价于按钮原生激活，键盘用户仍可操作）；
 * - 焦点不在任何按钮上 → 回车按确认处理。
 *
 * 统一 preventDefault 是为了阻止按钮原生点击，保证一次回车只派发一个事件；
 * stopPropagation 让底层工作区不再处理同一按键。
 */
function handleKeydown(event: KeyboardEvent): void {
  if (!props.open) {
    return;
  }
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    emit("cancel");
    return;
  }
  if (event.key !== "Enter") {
    return;
  }
  event.preventDefault();
  event.stopPropagation();
  // 只按「是否聚焦取消按钮」分流：其余焦点位置（确认按钮、对话框、窗口）都视为确认。
  if (event.target === cancelButton.value) {
    emit("cancel");
  } else {
    emit("confirm");
  }
}

// 监听挂在 window 上：焦点即使临时跑到按钮之外，键盘操作依然可用。
onMounted(() => window.addEventListener("keydown", handleKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", handleKeydown));
</script>

<template>
  <div v-if="open" class="overlay-backdrop flex items-center justify-center p-4" @click.self="emit('cancel')">
    <div
      class="modal-panel w-full max-w-[420px] p-5"
      role="dialog"
      aria-modal="true"
      :aria-labelledby="titleId"
      :aria-describedby="bodyId"
    >
      <h2 :id="titleId" class="text-[14px] font-semibold">{{ title }}</h2>
      <p :id="bodyId" class="mt-2 text-[12px] leading-relaxed text-ink-2">{{ body }}</p>
      <footer class="mt-5 flex justify-end gap-2">
        <button ref="cancelButton" type="button" class="ghost-btn" @click="emit('cancel')">取消</button>
        <button type="button" class="primary-btn" :class="{ 'danger-btn': danger }" @click="emit('confirm')">
          {{ confirmLabel }}
        </button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
/* 危险确认按钮：沿用 primary-btn 的尺寸与排版，只替换配色语义。 */
.danger-btn {
  background: var(--qc-danger);
}

.danger-btn:hover {
  background: var(--qc-danger);
  filter: brightness(0.92);
}
</style>
