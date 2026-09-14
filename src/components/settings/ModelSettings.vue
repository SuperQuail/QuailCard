<script setup lang="ts">
import { Check, LoaderCircle, LogIn, LogOut, Pencil, Plus } from "@lucide/vue";
import { onBeforeUnmount, ref } from "vue";
import { isTauri } from "../../api/backend";
import { resolveError } from "../../utils/errorMessage";
import { deleteProvider, getOpenAiLoginStatus, logoutOpenAi, startOpenAiLogin } from "../../services/stores/providerStore";
import type { ProviderSummary } from "../../domain/types";
import ProviderForm from "./ProviderForm.vue";

/**
 * 模型设置：每个供应商是一张卡片，编辑时就地展开表单（与 DSH 的布局一致）。
 *
 * 契约：本组件只负责列表、卡片状态与 OAuth 动作；
 * 表单字段与模型目录由 ProviderForm 拥有，活动供应商切换通过 emit 上抛。
 */
defineProps<{
  providers: ProviderSummary[];
  activeProviderId: string;
}>();

const emit = defineEmits<{
  "set-active-provider": [providerId: string];
}>();

/** 正在就地编辑的供应商 id；null 表示没有展开的表单。 */
const editingId = ref<string | null>(null);
/** 是否正在新增供应商。 */
const creating = ref(false);
const deletingProviderId = ref<string | null>(null);
const listMessage = ref("");
let deleteTimer: number | undefined;

/** OAuth 登录状态。 */
const oauthBusy = ref(false);
const oauthMessage = ref("");
let oauthTimer: number | undefined;

/** 返回供应商状态文案。 */
function providerStatus(provider: ProviderSummary): string {
  if (provider.status === "connected") {
    return "已连接";
  }
  return provider.hasApiKey || provider.hasCredential ? "已配置" : "未测试";
}

/** 状态点颜色：已连接为绿，仅配置了凭据为强调色，其余保持中性。 */
function statusClass(provider: ProviderSummary): string {
  if (provider.status === "connected") {
    return "bg-success";
  }
  return provider.hasApiKey || provider.hasCredential ? "bg-accent-strong" : "bg-hairline";
}

/** 卡片副标题：协议 · 当前模型（多模型时给出数量）· 状态。 */
function summaryLine(provider: ProviderSummary): string {
  const count = provider.models.length > 1 ? ` 等 ${provider.models.length} 个模型` : "";
  return `${provider.protocol} · ${provider.model}${count} · ${providerStatus(provider)}`;
}

/** 打开/收起某张卡片的就地编辑表单。 */
function toggleEditing(providerId: string): void {
  creating.value = false;
  editingId.value = editingId.value === providerId ? null : providerId;
}

/** 两段式删除：第一次点击进入确认，三秒内再点执行。 */
function requestDeleteProvider(providerId: string): void {
  if (deletingProviderId.value === providerId) {
    if (deleteTimer) {
      window.clearTimeout(deleteTimer);
    }
    deletingProviderId.value = null;
    listMessage.value = "";
    void deleteProvider(providerId).then(() => {
      listMessage.value = "已删除";
      if (editingId.value === providerId) {
        editingId.value = null;
      }
    }).catch((error) => {
      listMessage.value = resolveError(error);
    });
    return;
  }
  deletingProviderId.value = providerId;
  if (deleteTimer) {
    window.clearTimeout(deleteTimer);
  }
  deleteTimer = window.setTimeout(() => {
    deletingProviderId.value = null;
  }, 3000);
}

/** 注销 OpenAI 订阅登录。 */
async function logoutSubscription(providerId: string): Promise<void> {
  await logoutOpenAi(providerId);
  oauthMessage.value = "已注销";
}

/** 启动 OpenAI 订阅登录并轮询状态。 */
async function startOauthLogin(): Promise<void> {
  if (oauthBusy.value) {
    return;
  }
  oauthBusy.value = true;
  oauthMessage.value = "正在启动浏览器登录…";
  try {
    const attempt = await startOpenAiLogin("openai_subscription", "browser");
    if (isTauri()) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(attempt.url);
    } else {
      window.open(attempt.url, "_blank");
    }
    oauthTimer = window.setInterval(async () => {
      try {
        const status = await getOpenAiLoginStatus(attempt.attemptId);
        if (status.status === "success") {
          window.clearInterval(oauthTimer);
          oauthBusy.value = false;
          oauthMessage.value = "登录成功";
          return;
        }
        if (status.status === "failed" || status.status === "cancelled") {
          window.clearInterval(oauthTimer);
          oauthBusy.value = false;
          oauthMessage.value = status.message || "登录失败";
          return;
        }
        oauthMessage.value = status.message || "等待浏览器授权…";
      } catch (error) {
        window.clearInterval(oauthTimer);
        oauthBusy.value = false;
        oauthMessage.value = resolveError(error);
      }
    }, 2000);
  } catch (error) {
    oauthBusy.value = false;
    oauthMessage.value = resolveError(error);
  }
}

/** 卸载时清理轮询与删除确认计时器，避免遗留异步任务。 */
onBeforeUnmount(() => {
  if (deleteTimer) {
    window.clearTimeout(deleteTimer);
  }
  if (oauthTimer) {
    window.clearInterval(oauthTimer);
  }
});
</script>

<template>
  <h2 class="text-[16px] font-semibold">模型</h2>
  <p class="mt-1 text-[11px] text-ink-3">填入各提供方的 API 密钥即可使用其模型；每个模型可单独设置上下文窗口与最大输出 token。</p>

  <section class="mt-4 flex flex-col gap-3">
    <article v-for="provider in providers" :key="provider.id" class="rounded-lg border border-hairline">
      <header class="flex items-center gap-2 px-3 py-2">
        <button type="button" class="flex min-w-0 flex-1 items-center gap-2.5 text-left" title="设为活动供应商" @click="emit('set-active-provider', provider.id)">
          <span class="grid size-6 shrink-0 place-items-center rounded-full border border-hairline text-[9px] font-bold">{{ provider.name.slice(0, 1) }}</span>
          <span class="min-w-0">
            <span class="flex items-center gap-1.5 truncate text-[12px] font-medium">
              {{ provider.name }}
              <span class="truncate text-[10px] font-normal text-ink-3">{{ provider.id }}</span>
              <span class="size-1.5 shrink-0 rounded-full" :class="statusClass(provider)" />
              <Check v-if="activeProviderId === provider.id" :size="13" class="shrink-0 text-accent-strong" />
            </span>
            <span class="block truncate text-[10px] text-ink-3">{{ summaryLine(provider) }}</span>
          </span>
        </button>
        <span class="flex shrink-0 items-center gap-0.5">
          <button type="button" class="ghost-btn border border-hairline" @click="toggleEditing(provider.id)">
            <Pencil :size="12" />{{ editingId === provider.id ? "收起" : "编辑" }}
          </button>
          <button
            type="button"
            class="ghost-btn !text-danger"
            :title="deletingProviderId === provider.id ? '再点一次确认删除' : '删除'"
            @click="requestDeleteProvider(provider.id)"
          >
            {{ deletingProviderId === provider.id ? "确认删除" : "删除" }}
          </button>
        </span>
      </header>
      <div v-if="editingId === provider.id" class="px-3">
        <ProviderForm :editing="provider" @close="editingId = null">
          <template v-if="provider.providerType === 'openai_subscription'" #authentication>
            <div class="flex flex-col items-start gap-2">
              <span class="text-[11px] text-ink-2">OpenAI 订阅登录</span>
              <button v-if="!provider.hasCredential" type="button" class="ghost-btn border border-hairline" :disabled="oauthBusy" @click="startOauthLogin">
                <LoaderCircle v-if="oauthBusy" :size="14" class="animate-spin" />
                <LogIn v-else :size="14" />登录 OpenAI
              </button>
              <button v-else type="button" class="ghost-btn border border-hairline" :disabled="oauthBusy" @click="void logoutSubscription(provider.id)">
                <LogOut :size="14" />注销登录
              </button>
              <p v-if="oauthMessage" class="text-[11px] text-ink-2">{{ oauthMessage }}</p>
            </div>
          </template>
        </ProviderForm>
      </div>
    </article>

    <p v-if="listMessage" class="text-[11px] text-success">{{ listMessage }}</p>

    <!-- 新增供应商：没有既有卡片可展开，因此单独给一张表单卡片 -->
    <ProviderForm v-if="creating" :editing="null" @close="creating = false" />
    <button v-else type="button" class="ghost-btn self-start border border-hairline" @click="creating = true; editingId = null">
      <Plus :size="13" />新增供应商
    </button>
  </section>
</template>
