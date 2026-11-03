<!--
  右栏容器：卡片 / 大纲 / 反链三个同级页签。
  页签集合是数据（tabs 数组），新增一栏只需加一条数据并提供一个面板组件，
  不需要在模板里堆新的 if 分支。
-->
<script setup lang="ts">
import CardPanel from "../CardPanel.vue";
import SideTabs from "./SideTabs.vue";
import OutlinePanel from "./OutlinePanel.vue";
import BacklinksPanel from "./BacklinksPanel.vue";
import { computed } from "vue";
import { buildOutline } from "../../domain/outline";
import type { Backlink, NoteCard } from "../../domain/types";
import { sidePanelTab } from "../../services/stores/uiStore";

const props = defineProps<{
  cards: NoteCard[];
  /** 正文：大纲在这里现算，避免编辑时每次按键都让父层重算。 */
  content: string;
  activeCardId: string | null;
  /** 编辑器视口顶部的字符偏移；据此在大纲里定位当前小节。 */
  viewportOffset?: number | null;
  backlinks: Backlink[];
  /** 当前打开的是单词组：页签只保留「单词」，卡片区只读。 */
  wordGroup?: boolean;
}>();

const emit = defineEmits<{
  "edit-card": [cardId: string];
  "delete-card": [cardId: string];
  "add-card": [];
  "open-ai-split": [];
  "start-review": [];
  "start-typing": [];
  jump: [from: number];
  "open-backlink": [path: string, line: number];
}>();

/** 页签定义：单词组没有 Markdown 大纲与双链，只保留单词页签。 */
const tabs = computed(() => props.wordGroup
  ? [{ id: "cards", label: "单词" }]
  : [
    { id: "cards", label: "卡片" },
    { id: "outline", label: "大纲" },
    { id: "backlinks", label: "反链" },
  ]);

/** 单词组强制停在单词页签，避免沿用 Markdown 笔记记住的大纲/反链栏位。 */
const activeTab = computed(() => (props.wordGroup ? "cards" : sidePanelTab.value));

/** 大纲随正文变化重算；解析成本与正文长度线性相关，仅在右侧栏可见时才有消费者。 */
const outline = computed(() => buildOutline(props.content));

/**
 * 当前高亮的大纲项：取最后一个起点不晚于视口顶部的标题。
 * 只依赖偏移比较，ATX/Setext 混排和代码块里的 # 都不会造成误判。
 */
const activeHeading = computed<number | null>(() => {
  const offset = props.viewportOffset;
  if (offset === null || offset === undefined) return null;
  let active: number | null = null;
  for (const item of outline.value) {
    if (item.from > offset) break;
    active = item.from;
  }
  return active;
});

/** 切换页签：uiStore 只接受三个合法值。 */
function selectTab(id: string): void {
  if (id === "cards" || id === "outline" || id === "backlinks") {
    sidePanelTab.value = id;
  }
}
</script>

<template>
  <div class="flex h-full flex-col">
    <SideTabs :tabs="tabs" :active="activeTab" @select="selectTab" />

    <div class="min-h-0 flex-1">
      <CardPanel
        v-if="activeTab === 'cards'"
        :cards="cards"
        :active-card-id="activeCardId"
        :word-group="wordGroup"
        @edit-card="emit('edit-card', $event)"
        @delete-card="emit('delete-card', $event)"
        @add-card="emit('add-card')"
        @open-ai-split="emit('open-ai-split')"
        @start-review="emit('start-review')"
        @start-typing="emit('start-typing')"
      />
      <OutlinePanel
        v-else-if="activeTab === 'outline'"
        :items="outline"
        :active-from="activeHeading"
        @jump="emit('jump', $event)"
      />
      <BacklinksPanel
        v-else
        :backlinks="backlinks"
        @open="(path, line) => emit('open-backlink', path, line)"
      />
    </div>
  </div>
</template>
