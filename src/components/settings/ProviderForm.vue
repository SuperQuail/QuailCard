<script setup lang="ts">
import { LoaderCircle } from "@lucide/vue";
import { ref, watch } from "vue";
import { resolveError } from "../../utils/errorMessage";
import { saveProvider, testProvider } from "../../services/stores/providerStore";
import type { ProviderSummary } from "../../domain/types";

/**
 * 供应商新增/编辑表单：编辑时预填现有配置（API Key 留空表示不变）。
 * 保存与测试直接调用供应商域 store，结果消息本地呈现。
 */
const props = defineProps<{
  /** 编辑目标：null 表示新增。 */
  editing: ProviderSummary | null;
}>();

const emit = defineEmits<{
  close: [];
}>();

const name = ref("");
const protocol = ref("OpenAI Compatible");
const model = ref("");
const baseUrl = ref("");
const apiKey = ref("");
const message = ref("");
const busy = ref(false);

/** 编辑模式进入时预填现有配置（Key 不回显）。 */
watch(
  () => props.editing,
  (target) => {
    if (target) {
      name.value = target.name;
      protocol.value = target.protocol;
      model.value = target.model;
      baseUrl.value = target.baseUrl;
    }
  },
  { immediate: true },
);

/** 由当前表单值组装供应商输入。 */
function formInput() {
  return {
    id: props.editing?.id ?? null,
    name: name.value.trim(),
    shortCode: name.value.trim().slice(0, 2).toUpperCase(),
    protocol: protocol.value,
    model: model.value.trim(),
    baseUrl: baseUrl.value.trim() || "https://api.openai.com/v1",
    supportsVision: true,
    apiKey: apiKey.value.trim() || null,
  };
}

/** 保存供应商。 */
async function save(): Promise<void> {
  if (!name.value.trim() || !model.value.trim() || busy.value) {
    return;
  }
  busy.value = true;
  message.value = "";
  try {
    const provider = await saveProvider(formInput());
    message.value = `${provider.name} 已保存`;
    emit("close");
  } catch (error) {
    message.value = resolveError(error);
  } finally {
    busy.value = false;
  }
}

/** 用当前表单配置测试连接。 */
async function test(): Promise<void> {
  if (!name.value.trim() || !model.value.trim() || busy.value) {
    return;
  }
  busy.value = true;
  message.value = "";
  try {
    const result = await testProvider(formInput());
    const suffix = props.editing ? "（保存后生效）" : "";
    message.value = `连接正常 · ${result.latencyMs} ms${suffix}`;
  } catch (error) {
    message.value = resolveError(error);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="mt-3 mb-4 flex max-w-[380px] flex-col gap-2 rounded-lg bg-bg-side p-3">
    <input v-model="name" class="field-input" placeholder="名称，例如 DeepSeek" />
    <select v-model="protocol" class="field-input">
      <option>OpenAI Compatible</option>
      <option>Anthropic Messages</option>
    </select>
    <input v-model="model" class="field-input" placeholder="模型名称，例如 deepseek-chat" />
    <input v-model="baseUrl" class="field-input" placeholder="BaseURL，例如 https://api.deepseek.com/v1" />
    <input v-model="apiKey" type="password" class="field-input" :placeholder="editing ? 'API Key（留空保持不变）' : 'API Key（可选）'" />
    <div class="flex items-center gap-2">
      <button type="button" class="primary-btn" :disabled="busy || !name.trim() || !model.trim()" @click="save">
        {{ editing ? "保存修改" : "保存供应商" }}
      </button>
      <button type="button" class="ghost-btn border border-hairline" :disabled="busy" @click="test">
        <LoaderCircle v-if="busy" :size="12" class="animate-spin" />测试
      </button>
      <button type="button" class="ghost-btn" @click="emit('close')">取消</button>
    </div>
    <p v-if="message" class="text-[11px] text-success">{{ message }}</p>
  </div>
</template>
