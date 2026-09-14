<script setup lang="ts">
import type { TableAlignment } from "../../markdown/types";
import RichText from "./RichText.vue";
const props = defineProps<{
  header: string[];
  rows: string[][];
  alignments: TableAlignment[];
  fallbackWord?: string;
}>();
const emit = defineEmits<{ link: [href: string] }>();
/** 列对齐由声明决定；缺列按左对齐，与编辑器里的表格预览保持一致。 */
function align(index: number): TableAlignment {
  return props.alignments[index] ?? "left";
}
</script>
<template>
  <div class="max-w-full overflow-x-auto">
    <table class="w-full border-collapse text-[15px]">
      <thead>
        <tr>
          <th
            v-for="(cell, index) in header"
            :key="index"
            class="border border-hairline bg-code-bg px-3 py-1.5 font-semibold"
            :style="{ textAlign: align(index) }"
          ><RichText :text="cell" :fallback-word="fallbackWord" @link="emit('link', $event)" /></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(row, rowIndex) in rows" :key="rowIndex">
          <td
            v-for="(cell, index) in row"
            :key="index"
            class="border border-hairline px-3 py-1.5 align-top"
            :style="{ textAlign: align(index) }"
          ><RichText :text="cell" :fallback-word="fallbackWord" @link="emit('link', $event)" /></td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
