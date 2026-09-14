<script setup lang="ts">
import { ChevronDown, ChevronRight, Trash2 } from "@lucide/vue";
import { computed, ref, watch } from "vue";
import { DEFAULT_MAX_OUTPUT_TOKENS, formatTokenCount, modelIssue, parseTokenCount } from "../../domain/providerModels";
import type { ProviderModel } from "../../domain/types";

/**
 * 模型目录中的一行：模型 ID、显示名称，展开后填写上下文窗口与最大输出 token。
 *
 * 契约：组件只收集输入并向上抛出 patch / remove / activate，
 * 不调用后端、不直接修改传入对象；空文本一律提交 null（表示未设置）。
 */
const props = defineProps<{
  model: ProviderModel;
  /** 是否为当前使用的模型，决定请求实际发给哪个模型。 */
  active: boolean;
  /** 目录至少要保留一项，因此最后一行不允许删除。 */
  removable: boolean;
}>();

const emit = defineEmits<{
  patch: [patch: Partial<ProviderModel>];
  remove: [];
  activate: [];
}>();

const expanded = ref(false);
const contextText = ref(formatTokenCount(props.model.contextWindow));
const outputText = ref(formatTokenCount(props.model.maxOutputTokens));

/** 外部改动数值时同步输入框（例如切换编辑目标），避免显示过期文本。 */
watch(
  () => props.model.contextWindow,
  (value) => {
    if (parseTokenCount(contextText.value) !== value) {
      contextText.value = formatTokenCount(value);
    }
  },
);
watch(
  () => props.model.maxOutputTokens,
  (value) => {
    if (parseTokenCount(outputText.value) !== value) {
      outputText.value = formatTokenCount(value);
    }
  },
);

/** 无法解析的文本立刻提示，并且不会把半截数字提交上去。 */
const contextInvalid = computed(
  () => contextText.value.trim() !== "" && parseTokenCount(contextText.value) === null,
);
const outputInvalid = computed(
  () => outputText.value.trim() !== "" && parseTokenCount(outputText.value) === null,
);
const issue = computed(() => modelIssue(props.model));

/** 两个数值输入共用的写回逻辑：先更新本地文本，再解析并向上抛出。 */
function onContextInput(event: Event): void {
  contextText.value = (event.target as HTMLInputElement).value;
  emit("patch", { contextWindow: parseTokenCount(contextText.value) });
}

function onOutputInput(event: Event): void {
  outputText.value = (event.target as HTMLInputElement).value;
  emit("patch", { maxOutputTokens: parseTokenCount(outputText.value) });
}
</script>

<template>
  <div class="flex items-start gap-2">
    <input
      type="radio"
      class="mt-2 shrink-0"
      name="provider-active-model"
      :checked="active"
      title="设为当前使用的模型"
      @change="emit('activate')"
    />
    <div class="flex min-w-0 flex-1 flex-col gap-1.5">
      <div class="flex items-center gap-1.5">
        <input
          class="field-input min-w-0 flex-1"
          :value="model.id"
          placeholder="模型 ID，例如 deepseek-chat"
          @input="emit('patch', { id: ($event.target as HTMLInputElement).value })"
        />
        <input
          class="field-input min-w-0 flex-1"
          :value="model.name"
          placeholder="显示名称（可留空）"
          @input="emit('patch', { name: ($event.target as HTMLInputElement).value })"
        />
        <button
          type="button"
          class="icon-btn"
          :title="expanded ? '收起' : '上下文窗口与输出上限'"
          @click="expanded = !expanded"
        >
          <ChevronDown v-if="expanded" :size="14" />
          <ChevronRight v-else :size="14" />
        </button>
        <button
          type="button"
          class="icon-btn hover:!text-danger"
          :disabled="!removable"
          title="删除该模型"
          @click="emit('remove')"
        >
          <Trash2 :size="14" />
        </button>
      </div>
      <div v-if="expanded" class="grid grid-cols-2 gap-2">
        <label class="flex flex-col gap-1 text-[10px] text-ink-3">
          上下文窗口
          <input :value="contextText" class="field-input" placeholder="256K" @input="onContextInput" />
          <span v-if="contextInvalid" class="text-danger">请输入数字，可带 K/M 后缀</span>
        </label>
        <label class="flex flex-col gap-1 text-[10px] text-ink-3">
          最大输出 token
          <input :value="outputText" class="field-input" :placeholder="String(DEFAULT_MAX_OUTPUT_TOKENS)" @input="onOutputInput" />
          <span v-if="outputInvalid" class="text-danger">请输入数字，可带 K/M 后缀</span>
        </label>
      </div>
      <p v-if="issue" class="text-[10px] text-danger">{{ issue }}</p>
    </div>
  </div>
</template>
