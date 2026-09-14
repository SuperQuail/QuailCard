<script setup lang="ts">
import { computed } from "vue";
import type { AgentMessage, AgentToolDetail } from "../../domain/agent";
const props = defineProps<{ message: AgentMessage }>();
const labels: Record<string, string> = { running: "运行中", ok: "成功", error: "失败" };
/** 仅展示后端投影的有界字段；不从旧协议或原始结果恢复内容。 */
function details(value: unknown): AgentToolDetail[] {
  if (!Array.isArray(value)) return [];
  return value.slice(0, 20).flatMap((item: unknown) => {
    if (!item || typeof item !== "object") return [];
    const field = item as Record<string, unknown>;
    if (typeof field.label !== "string" || typeof field.value !== "string") return [];
    return [{ label: field.label.slice(0, 60), value: field.value.slice(0, 300) }];
  });
}
/** 只消费公开 DTO，畸形状态、重放、任意 arguments/output 均不进入 DOM。 */
const rows = computed(() => {
  const raw = props.message.kind === "tool_calls" ? props.message.data?.rows : null;
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item: unknown) => {
    if (!item || typeof item !== "object") return [];
    const row = item as Record<string, unknown>;
    if (typeof row.id !== "string" || typeof row.name !== "string" || typeof row.summary !== "string"
      || typeof row.state !== "string" || !Object.prototype.hasOwnProperty.call(labels, row.state)) return [];
    const errorCode = typeof row.errorCode === "string" && /^[A-Z][A-Z0-9_]{0,79}$/.test(row.errorCode) ? row.errorCode : "";
    return [{ id: row.id, name: row.name, summary: row.summary, state: row.state, details: details(row.details), errorCode }];
  });
});
</script>
<template>
  <div class="safe-tools" aria-label="工具执行摘要">
    <details v-for="(row, index) in rows" :key="row.id + index" :data-state="row.state" :open="row.state === 'error'">
      <summary>
        <span class="chevron" aria-hidden="true">›</span>
        <span class="tool-name">{{ row.name }}</span>
        <span class="status">{{ labels[row.state] }}</span>
        <span class="preview">{{ row.summary }}</span>
      </summary>
      <div class="tool-detail">
        <p v-if="row.errorCode" class="error-code">错误码 <code>{{ row.errorCode }}</code></p>
        <dl v-if="row.details.length">
          <template v-for="(field, fieldIndex) in row.details" :key="fieldIndex">
            <dt>{{ field.label }}</dt><dd>{{ field.value }}</dd>
          </template>
        </dl>
        <p v-else class="hint">此记录未保存可展示的参数或结果详情。</p>
        <p v-if="row.state === 'running'" class="hint" role="status">工具正在执行，结果会自动更新。</p>
      </div>
    </details>
    <p v-if="!rows.length" class="hint">{{ message.kind === 'exchange' ? '旧工具记录未提供安全摘要，原始协议已隐藏。' : '暂无安全工具摘要。' }}</p>
  </div>
</template>
<style scoped>
.safe-tools { display: grid; gap: 6px; font-size: 12px; }
details { border: 1px solid var(--color-hairline); border-radius: 8px; overflow: hidden; }
summary { display: flex; align-items: baseline; gap: 8px; padding: 8px 10px; cursor: pointer; list-style: none; }
summary::-webkit-details-marker { display: none; }
summary:focus-visible { outline: 2px solid var(--color-ink-3); outline-offset: -2px; }
.chevron { color: var(--color-ink-3); transition: transform .15s; }
details[open] .chevron { transform: rotate(90deg); }
.tool-name { font-family: monospace; overflow-wrap: anywhere; }
.status { flex-shrink: 0; }
.preview { flex: 1; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: var(--color-ink-3); }
details[open] .preview { white-space: normal; overflow-wrap: anywhere; }
.tool-detail { padding: 10px 12px; border-top: 1px solid var(--color-hairline); }
dl { display: grid; grid-template-columns: minmax(70px, auto) minmax(0, 1fr); gap: 6px 12px; margin: 0; }
dt { color: var(--color-ink-3); } dd { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
p { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
.error-code { margin-bottom: 8px; } code { user-select: all; }
.hint { color: var(--color-ink-3); } .hint + .hint { margin-top: 6px; }
[data-state="error"] summary, .error-code { color: var(--color-danger); }
</style>
