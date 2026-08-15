<script setup lang="ts">
import { Check, LoaderCircle, Sparkles, X } from "@lucide/vue";
import { onBeforeUnmount, ref } from "vue";
import { useAppState } from "../services/appState";
import type { CardKind } from "../domain/types";

/** AI 拆卡草稿：展示与采纳的统一形状。 */
export interface AiDraftCard {
  front: string;
  back: string;
  detail: string;
  example: string;
  rubric: string[];
  source: string;
}

const props = defineProps<{
  kind: CardKind;
  noteTitle: string;
  noteContent: string;
  hasSelection: boolean;
  selectionText: string;
  /** 是否已配置至少一个可用供应商。 */
  providerConfigured: boolean;
}>();

const emit = defineEmits<{
  close: [];
  adopt: [drafts: AiDraftCard[]];
  "open-settings": [];
  "error": [message: string];
}>();

const store = useAppState();

type Step = "scope" | "running" | "drafts";

const step = ref<Step>("scope");
const scope = ref<"note" | "selection">("note");
const countChoice = ref<string>("auto");
const streamLine = ref("");
const drafts = ref<AiDraftCard[]>([]);
const accepted = ref<Set<number>>(new Set());
let streamTimer: number | undefined;

/** 返回当前类型的名称。 */
function kindName(): string {
  if (props.kind === "vocabulary") {
    return "单词记忆";
  }
  return "问答卡";
}

/** 把卡片类型映射为后端生成参数。 */
function resolveGenerationParams(): { typeId: string; studyModeId: string } {
  if (props.kind === "vocabulary") {
    return { typeId: "vocabulary", studyModeId: "dictation" };
  }
  // 问答统一生成判定要点，供开启 AI 评分时使用。
  return { typeId: "qa", studyModeId: "ai-review" };
}

/** 开始拆卡：进度文案本地推进，真实生成走后端。 */
async function startSplit(): Promise<void> {
  if (!props.providerConfigured) {
    return; // 未配置时按钮已禁用，黄色提示框负责说明。
  }
  step.value = "running";
  streamLine.value = "";
  const sourceText = scope.value === "selection" && props.selectionText.trim() ? props.selectionText : props.noteContent;
  const lines = [
    "正在通读笔记，标记可复习的知识点…",
    `识别到笔记类型：${kindName()}`,
    scope.value === "selection" ? `聚焦选中段落：${props.selectionText.slice(0, 24)}…` : "按标题结构切分内容段落…",
    "为每个知识点生成问题与答案…",
  ];
  let lineIndex = 0;
  streamTimer = window.setInterval(() => {
    if (lineIndex < lines.length) {
      streamLine.value = lines[lineIndex];
      lineIndex += 1;
    }
  }, 480);

  const { typeId, studyModeId } = resolveGenerationParams();
  try {
    const result = await store.generateCards({
      typeId,
      studyModeId,
      noteTitle: props.noteTitle,
      sourceText,
      requestedCount: countChoice.value === "five" ? 5 : countChoice.value === "ten" ? 10 : -1,
    });
    finishSplit(result.cards);
  } catch (error) {
    emit("error", resolveError(error));
    step.value = "scope";
  } finally {
    if (streamTimer) {
      window.clearInterval(streamTimer);
    }
  }
}

/** 将生成结果转换为展示草稿。 */
function finishSplit(generated: Array<{ fields: Record<string, string> }>): void {
  drafts.value = generated.map((card) => ({
    front: card.fields.front ?? "",
    back: card.fields.back ?? "",
    detail: card.fields.detail ?? "",
    example: card.fields.example ?? "",
    rubric: (card.fields.rubric ?? "").split(/[、,，]/).map((item) => item.trim()).filter(Boolean),
    source: card.fields.source ?? "",
  }));
  accepted.value = new Set(drafts.value.map((_, index) => index));
  step.value = "drafts";
}

/** 切换某张草稿的采纳状态。 */
function toggleAccepted(index: number): void {
  const next = new Set(accepted.value);
  if (next.has(index)) {
    next.delete(index);
  } else {
    next.add(index);
  }
  accepted.value = next;
}

/** 删除某张草稿。 */
function removeDraft(index: number): void {
  drafts.value = drafts.value.filter((_, itemIndex) => itemIndex !== index);
  accepted.value = new Set(drafts.value.map((_, itemIndex) => itemIndex));
}

/** 提交采纳选中的草稿。 */
function adoptDrafts(): void {
  const selected = drafts.value.filter((_, index) => accepted.value.has(index));
  if (selected.length > 0) {
    emit("adopt", selected);
  }
}

/** 取消并关闭。 */
function cancel(): void {
  emit("close");
}

/** 提取错误消息。 */
function resolveError(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    return String(error.message);
  }
  return String(error);
}

onBeforeUnmount(() => {
  if (streamTimer) {
    window.clearInterval(streamTimer);
  }
});
</script>

<template>
  <div class="overlay-backdrop flex items-start justify-center pt-20" @click.self="cancel">
    <div class="modal-panel w-full max-w-[480px] p-5">
      <!-- 标题 -->
      <header class="mb-4 flex items-center justify-between">
        <h2 class="flex items-center gap-2 text-[14px] font-semibold">
          <Sparkles :size="15" class="text-accent-strong" />AI 拆卡
        </h2>
        <button type="button" class="icon-btn" aria-label="关闭" @click="cancel">
          <X :size="15" />
        </button>
      </header>

      <!-- 第一步：范围与数量 -->
      <template v-if="step === 'scope'">
        <!-- 未配置供应商提示 -->
        <div v-if="!providerConfigured" class="mb-3 flex items-center justify-between gap-3 rounded-lg border border-warning/40 bg-warning/8 px-3 py-2.5">
          <p class="text-[11px] leading-5 text-ink-2">还没有配置可用的 AI 供应商，拆卡需要先在设置中配置模型与凭据。</p>
          <button type="button" class="ghost-btn shrink-0 border border-hairline" @click="emit('open-settings')">打开设置</button>
        </div>

        <div class="mb-3 grid grid-cols-2 gap-2">
          <button type="button" class="rounded-lg border p-3 text-left transition" :class="scope === 'note' ? 'border-accent bg-bg-active/40' : 'border-hairline hover:border-accent/50'" @click="scope = 'note'">
            <p class="text-[12px] font-medium">整篇笔记</p>
            <p class="mt-0.5 text-[10px] text-ink-3">通读全文提炼知识点</p>
          </button>
          <button type="button" class="rounded-lg border p-3 text-left transition" :class="scope === 'selection' ? 'border-accent bg-bg-active/40' : 'border-hairline hover:border-accent/50'" :disabled="!hasSelection" @click="scope = 'selection'">
            <p class="text-[12px] font-medium">选中段落</p>
            <p class="mt-0.5 text-[10px] text-ink-3">{{ hasSelection ? selectionText.slice(0, 18) + "…" : "先在正文中选中一段话" }}</p>
          </button>
        </div>
        <div class="mb-4 flex items-center gap-1.5">
          <button v-for="option in [{ id: 'auto', label: '自动' }, { id: 'five', label: '5 张' }, { id: 'ten', label: '10 张' }]" :key="option.id" type="button" class="rounded-full border px-3 py-1 text-[11px] transition" :class="countChoice === option.id ? 'border-accent bg-bg-active/40 text-accent-strong' : 'border-hairline text-ink-2 hover:border-accent/50'" @click="countChoice = option.id">
            {{ option.label }}
          </button>
        </div>
        <footer class="flex justify-end gap-2">
          <button type="button" class="ghost-btn" @click="cancel">取消</button>
          <button type="button" class="primary-btn" :disabled="!providerConfigured" @click="startSplit">
            <Sparkles :size="14" />开始拆卡
          </button>
        </footer>
      </template>

      <!-- 第二步：生成中 -->
      <template v-else-if="step === 'running'">
        <div class="flex flex-col items-center py-8">
          <LoaderCircle :size="22" class="animate-spin text-accent-strong" />
          <p class="stream-caret mt-4 min-h-5 text-center text-[12px] text-ink-2">{{ streamLine || "准备中…" }}</p>
        </div>
        <footer class="flex justify-end">
          <button type="button" class="ghost-btn" @click="cancel">取消</button>
        </footer>
      </template>

      <!-- 第三步：草稿确认 -->
      <template v-else>
        <p class="mb-3 text-[11px] text-ink-3">生成 {{ drafts.length }} 张草稿，勾选要保留的卡片：</p>
        <ul class="soft-scrollbar max-h-[46vh] space-y-2 overflow-y-auto pr-1">
          <li v-for="(draft, index) in drafts" :key="index" class="rounded-lg border border-hairline bg-bg-paper p-3">
            <div class="flex items-start gap-2.5">
              <button type="button" class="mt-0.5 grid size-4 shrink-0 place-items-center rounded border transition" :class="accepted.has(index) ? 'border-accent bg-accent text-white' : 'border-ink-3'" aria-label="保留这张卡片" @click="toggleAccepted(index)">
                <Check v-if="accepted.has(index)" :size="11" />
              </button>
              <div class="min-w-0 flex-1">
                <p class="text-[12px] leading-5 font-medium">{{ draft.front }}</p>
                <p class="mt-0.5 line-clamp-2 text-[11px] leading-4 text-ink-3">{{ draft.back }}</p>
                <p class="mt-1 text-[10px] text-ink-3">来源：{{ draft.source.slice(0, 30) }}…</p>
              </div>
              <button type="button" class="icon-btn !size-6 shrink-0" title="移除这张草稿" @click="removeDraft(index)">
                <X :size="12" />
              </button>
            </div>
          </li>
        </ul>
        <footer class="mt-4 flex justify-end gap-2">
          <button type="button" class="ghost-btn" @click="cancel">放弃全部</button>
          <button type="button" class="primary-btn" :disabled="drafts.length === 0" @click="adoptDrafts">
            <Check :size="14" />采纳 {{ [...accepted].length }} 张
          </button>
        </footer>
      </template>
    </div>
  </div>
</template>
