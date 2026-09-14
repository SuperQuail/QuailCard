<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import katex from "katex";
const props = defineProps<{ tex: string; display?: boolean }>();
const host = ref<HTMLElement | null>(null);
/** 用 KaTeX 直接渲染成 DOM 节点，不生成 HTML 字符串，保持"不解析任意 HTML"的约束。 */
function render(): void {
  const element = host.value;
  if (!element) {
    return;
  }
  element.textContent = "";
  try {
    katex.render(props.tex, element, { displayMode: Boolean(props.display), throwOnError: false });
  } catch {
    element.textContent = props.tex;
  }
}
onMounted(render);
watch(() => [props.tex, props.display], render);
</script>
<template>
  <span ref="host" class="qc-math" :class="{ 'is-display': display }" :title="tex" />
</template>
