<script setup lang="ts">
import { BookOpen, Type, X } from "@lucide/vue";
import { ref, watch } from "vue";
import type { CardKind } from "../domain/types";

const props = defineProps<{
  kind: CardKind;
  editingId: string | null;
  front: string;
  back: string;
  detail: string;
  example: string;
  rubric: string;
}>();

const emit = defineEmits<{
  close: [];
  save: [{ kind: CardKind; front: string; back: string; detail: string; example: string; rubric: string }];
}>();

const localKind = ref<CardKind>(props.kind);
const front = ref(props.front);
const back = ref(props.back);
const detail = ref(props.detail);
const example = ref(props.example);
const rubric = ref(props.rubric);

/** 打开时同步外部初始内容。 */
watch(
  () => [props.kind, props.front, props.back, props.detail, props.example, props.rubric],
  () => {
    localKind.value = props.kind;
    front.value = props.front;
    back.value = props.back;
    detail.value = props.detail;
    example.value = props.example;
    rubric.value = props.rubric;
  },
  { immediate: true },
);

/** 返回当前类型的正面标签。 */
function frontLabel(): string {
  if (localKind.value === "vocabulary") {
    return "释义";
  }
  return "问题";
}

/** 返回当前类型的背面标签。 */
function backLabel(): string {
  if (localKind.value === "vocabulary") {
    return "单词";
  }
  return "参考答案";
}

/** 提交保存并关闭。 */
function submit(): void {
  if (!front.value.trim() || !back.value.trim()) {
    return;
  }
  emit("save", {
    kind: localKind.value,
    front: front.value.trim(),
    back: back.value.trim(),
    detail: detail.value.trim(),
    example: example.value.trim(),
    rubric: rubric.value.trim(),
  });
}
</script>

<template>
  <div class="overlay-backdrop flex items-start justify-center pt-24" @click.self="emit('close')">
    <div class="modal-panel w-full max-w-[420px] p-5">
      <header class="mb-4 flex items-center justify-between">
        <h2 class="text-[14px] font-semibold">{{ editingId ? "编辑卡片" : "拆成卡片" }}</h2>
        <button type="button" class="icon-btn" aria-label="关闭" @click="emit('close')">
          <X :size="15" />
        </button>
      </header>

      <!-- 卡片类型切换 -->
      <div class="mb-4 grid grid-cols-2 gap-1 rounded-lg bg-bg-side p-1">
        <button
          v-for="option in [
            { id: 'vocabulary', label: '单词', icon: Type },
            { id: 'qa', label: '问答', icon: BookOpen },
          ]"
          :key="option.id"
          type="button"
          class="flex items-center justify-center gap-1 rounded-md py-1.5 text-[11px] font-medium transition"
          :class="localKind === option.id ? 'bg-bg-paper text-accent-strong shadow-sm' : 'text-ink-3 hover:text-ink-2'"
          @click="localKind = option.id as CardKind"
        >
          <component :is="option.icon" :size="12" />{{ option.label }}
        </button>
      </div>

      <label class="mb-3 block">
        <span class="mb-1 block text-[11px] font-medium text-ink-2">{{ frontLabel() }}</span>
        <textarea v-model="front" class="field-textarea" :rows="2" :placeholder="localKind === 'vocabulary' ? '输入释义，例如：短暂的；转瞬即逝的' : '输入问题'" autofocus />
      </label>

      <label class="mb-3 block">
        <span class="mb-1 block text-[11px] font-medium text-ink-2">{{ backLabel() }}</span>
        <textarea v-model="back" class="field-textarea" :rows="2" :placeholder="localKind === 'vocabulary' ? '输入单词' : '输入参考答案'" />
      </label>

      <label v-if="localKind === 'vocabulary'" class="mb-3 block">
        <span class="mb-1 block text-[11px] font-medium text-ink-2">音标</span>
        <input v-model="detail" class="field-input" placeholder="/ˈwɜːrd/" />
      </label>

      <label v-if="localKind === 'vocabulary'" class="mb-3 block">
        <span class="mb-1 block text-[11px] font-medium text-ink-2">例句</span>
        <textarea v-model="example" class="field-textarea" :rows="2" placeholder="写一个自然语境中的例句" />
      </label>

      <label v-else class="mb-3 block">
        <span class="mb-1 block text-[11px] font-medium text-ink-2">来源说明</span>
        <input v-model="detail" class="field-input" placeholder="例如：来源章节或页码" />
      </label>

      <footer class="flex justify-end gap-2">
        <button type="button" class="ghost-btn" @click="emit('close')">取消</button>
        <button type="button" class="primary-btn" :disabled="!front.trim() || !back.trim()" @click="submit">
          {{ editingId ? "保存修改" : "创建卡片" }}
        </button>
      </footer>
    </div>
  </div>
</template>
