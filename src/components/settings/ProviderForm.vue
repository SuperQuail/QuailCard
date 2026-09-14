<script setup lang="ts">
import { LoaderCircle, Plus } from "@lucide/vue";
import { computed, ref, watch } from "vue";
import { modelIssue, normalizeModels } from "../../domain/providerModels";
import { resolveError } from "../../utils/errorMessage";
import { saveProvider, testProvider } from "../../services/stores/providerStore";
import type { ProviderModel, ProviderSummary } from "../../domain/types";
import ProviderModelRow from "./ProviderModelRow.vue";

/**
 * 供应商新增/编辑表单：基础配置 → 认证 → 图片能力 → 模型目录，常用字段直接可见。
 *
 * 契约：
 * - 模型目录整表提交，model 字段同步为当前选中的模型 id（后端据此选模型）；
 * - API Key 留空表示保持原密文不变；
 * - 图片能力是显式选项，纯文本模型必须能关掉，否则带图请求只会在调用时才失败。
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
const baseUrl = ref("");
/** 新增供应商默认按支持图片处理，与旧的固定行为一致；编辑时以已保存值为准。 */
const supportsVision = ref(true);
const apiKey = ref("");
const models = ref<ProviderModel[]>([]);
/** 当前使用的模型下标；保存时换算成 model 字段。 */
const activeIndex = ref(0);
const message = ref("");
const busy = ref(false);

/** API 地址留空时会写入的默认端点；占位符必须显示真实默认值，不能写别的示例。 */
const DEFAULT_BASE_URL = "https://api.openai.com/v1";

/** 新行的初始值：只有模型 ID 必填，两个上限留空即按后端默认。 */
function emptyModel(): ProviderModel {
  return { id: "", name: "", contextWindow: null, maxOutputTokens: null };
}

/** 编辑模式进入时预填现有配置（Key 不回显）；旧配置只有单个 model 时补成一项目录。 */
watch(
  () => props.editing,
  (target) => {
    apiKey.value = "";
    message.value = "";
    if (!target) {
      name.value = "";
      protocol.value = "OpenAI Compatible";
      baseUrl.value = "";
      supportsVision.value = true;
      models.value = [emptyModel()];
      activeIndex.value = 0;
      return;
    }
    name.value = target.name;
    protocol.value = target.protocol;
    baseUrl.value = target.baseUrl;
    supportsVision.value = target.supportsVision;
    models.value = target.models.length
      ? target.models.map((model) => ({ ...model }))
      : [{ id: target.model, name: target.model, contextWindow: null, maxOutputTokens: null }];
    const index = models.value.findIndex((model) => model.id === target.model);
    activeIndex.value = index >= 0 ? index : 0;
  },
  { immediate: true },
);

/** 模型 ID 重复会让“当前使用哪一个”含糊，因此与空 ID 一样阻止保存。 */
const duplicated = computed(() => {
  const ids = models.value.map((model) => model.id.trim()).filter(Boolean);
  return new Set(ids).size !== ids.length;
});

/** 目录为空、ID 重复或数值越界都不允许提交，具体原因由行内提示。 */
const invalid = computed(
  () => models.value.length === 0 || duplicated.value || models.value.some((model) => modelIssue(model) !== null),
);

/** 更新某一行；行内组件只抛增量字段。 */
function patchModel(index: number, patch: Partial<ProviderModel>): void {
  const current = models.value[index];
  if (!current) {
    return;
  }
  models.value[index] = { ...current, ...patch };
}

/** 新增一行空模型，交回给用户填写。 */
function addModel(): void {
  models.value.push(emptyModel());
}

/** 删除一行；目录至少保留一项，并修正选中下标。 */
function removeModel(index: number): void {
  if (models.value.length <= 1) {
    return;
  }
  models.value.splice(index, 1);
  if (activeIndex.value === index) {
    activeIndex.value = 0;
  } else if (activeIndex.value > index) {
    activeIndex.value -= 1;
  }
}

/** 由当前表单值组装供应商输入：目录整表替换，model 指向选中项。 */
function formInput() {
  const normalized = normalizeModels(models.value);
  const active = normalized[activeIndex.value] ?? normalized[0];
  return {
    id: props.editing?.id ?? null,
    name: name.value.trim(),
    shortCode: name.value.trim().slice(0, 2).toUpperCase(),
    protocol: protocol.value,
    model: active?.id ?? "",
    models: normalized,
    baseUrl: baseUrl.value.trim() || DEFAULT_BASE_URL,
    supportsVision: supportsVision.value,
    apiKey: apiKey.value.trim() || null,
  };
}

/** 保存供应商。 */
async function save(): Promise<void> {
  if (!name.value.trim() || invalid.value || busy.value) {
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
  if (!name.value.trim() || invalid.value || busy.value) {
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
  <div class="mt-3 mb-4 flex flex-col gap-3 rounded-lg bg-bg-side p-3">
    <!-- 常用配置直接展示，避免编辑时反复展开折叠区。 -->
    <p class="text-[12px] font-semibold">{{ name.trim() || "新供应商" }}</p>

    <label class="flex flex-col gap-1 text-[11px] text-ink-2">
      显示名称
      <input v-model="name" class="field-input" placeholder="名称，例如 DeepSeek" />
    </label>
    <label class="flex flex-col gap-1 text-[11px] text-ink-2">
      API 地址
      <input v-model="baseUrl" class="field-input" :placeholder="DEFAULT_BASE_URL" />
    </label>
    <label class="flex flex-col gap-1 text-[11px] text-ink-2">
      API 协议
      <select v-model="protocol" class="field-input">
        <option>OpenAI Compatible</option>
        <option>Anthropic Messages</option>
      </select>
    </label>

    <!-- 认证入口由父层注入，普通供应商默认使用 API 密钥。 -->
    <slot name="authentication">
      <label class="flex flex-col gap-1 text-[11px] text-ink-2">
        API 密钥
        <input
          v-model="apiKey"
          type="password"
          class="field-input"
          :placeholder="editing?.hasCredential ? '已配置——输入新值可替换' : '粘贴 API 密钥'"
        />
      </label>
    </slot>

    <label class="flex items-center gap-2 text-[11px] text-ink-2">
      <input v-model="supportsVision" type="checkbox" />
      支持图片输入（视觉）
      <span class="text-ink-3">可读图片；关闭后带图请求会被拒绝</span>
    </label>

    <div class="flex flex-col gap-2">
      <div class="flex items-baseline justify-between">
        <span class="text-[11px] font-semibold tracking-wide text-ink-3 uppercase">模型目录</span>
        <span class="text-[10px] text-ink-3">选中的模型用于对话；展开可设上下文窗口与输出上限</span>
      </div>
      <ProviderModelRow
        v-for="(model, index) in models"
        :key="index"
        :model="model"
        :active="index === activeIndex"
        :removable="models.length > 1"
        @patch="patchModel(index, $event)"
        @remove="removeModel(index)"
        @activate="activeIndex = index"
      />
      <p v-if="duplicated" class="text-[10px] text-danger">模型 ID 不能重复</p>
      <button type="button" class="ghost-btn self-start border border-hairline" @click="addModel">
        <Plus :size="13" />添加模型
      </button>
    </div>

    <!-- 与 DSH 一致：取消在左、保存在右，主操作靠右收尾 -->
    <div class="flex items-center justify-end gap-2">
      <button type="button" class="ghost-btn mr-auto border border-hairline" :disabled="busy" @click="test">
        <LoaderCircle v-if="busy" :size="12" class="animate-spin" />测试
      </button>
      <button type="button" class="ghost-btn border border-hairline" @click="emit('close')">取消</button>
      <button type="button" class="primary-btn" :disabled="busy || !name.trim() || invalid" @click="save">
        {{ editing ? "保存" : "保存供应商" }}
      </button>
    </div>
    <p v-if="message" class="text-[11px] text-success">{{ message }}</p>
  </div>
</template>
