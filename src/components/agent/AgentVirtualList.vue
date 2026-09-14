<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

/**
 * 通用窗口化列表（虚拟滚动）：行高固定，只渲染可视窗口 ± overscan 的行，
 * 窗口之外用上下两个 aria-hidden spacer 撑起高度，滚动条长度与真实内容一致。
 * 纯展示组件：不认识任何业务字段，行内容由父级 scoped slot 按 index 渲染。
 * 做法参考 DSH TrajectoryTable：估算行高 + overscan + 上下 spacer 撑高。
 */
const props = withDefaults(
  defineProps<{
    /** 总行数；0 表示空列表，data-virtual-* 仍然输出，便于测试与调试。 */
    count: number;
    /** 每行固定像素高度：撑高与窗口换算都以它为准，行内容不得超出。 */
    itemHeight: number;
    /** 滚动视口固定高度；总高不超过它时根本不需要滚动，直接全量渲染。 */
    viewportHeight: number;
    /** 窗口上下各多渲染的缓冲行数，减少快速滚动时的露白。 */
    overscan?: number;
    /** 行数不超过它时也全量渲染，避免小列表承担虚拟化的复杂度。 */
    minWindow?: number;
    /** 列表语义标签：同一页面有多个列表时读屏用户靠它区分。 */
    ariaLabel?: string;
  }>(),
  { overscan: 4, minWindow: 6 },
);

/** 滚动容器：只有它能给出真实 scrollTop，也是把夹紧结果写回 DOM 的出口。 */
const container = ref<HTMLElement | null>(null);
/** 已提交的滚动位置；scroll 事件只登记一帧，rAF 回调才更新它，避免逐像素重排。 */
const scrollTop = ref(0);
/** 行高与视口必须为正数，否则窗口换算会出现除零或负高度。 */
const rowHeight = computed(() => Math.max(1, props.itemHeight));
/** 容器被 flex 压缩或拉伸时 prop 会失真；测到真实高度就以它为准。 */
const measured = ref(0);
const viewport = computed(() => Math.max(1, measured.value || props.viewportHeight));
/** 行数不超 minWindow 或总高不超视口时不需要滚动，此时不渲染 spacer，start=0、end=count。 */
const virtual = computed(() => props.count > Math.max(0, props.minWindow) && props.count * rowHeight.value > viewport.value);
/** 窗口起点：向上多留 overscan 行；并夹到末行，列表变短时不会停在窗口外。 */
const start = computed(() => {
  if (!virtual.value) return 0;
  const first = Math.floor(scrollTop.value / rowHeight.value) - Math.max(0, props.overscan);
  return Math.min(Math.max(0, first), Math.max(0, props.count - 1));
});
/** 窗口终点：向下多留 overscan 行；不超过 count，也不早于 start（空窗口即不渲染行）。 */
const end = computed(() => {
  if (!virtual.value) return props.count;
  const last = Math.ceil((scrollTop.value + viewport.value) / rowHeight.value) + Math.max(0, props.overscan);
  return Math.max(start.value, Math.min(props.count, last));
});
/** 窗口内行下标；key 用下标而非身份，滚动时才能复用同一批 DOM 节点。 */
const visible = computed(() => Array.from({ length: end.value - start.value }, (_, offset) => start.value + offset));
/** 上下 spacer 高度 = 被裁掉的行数 × 行高，两者相加保证滚动区域与全量渲染时等高。 */
const topSpacer = computed(() => (virtual.value ? start.value * rowHeight.value : 0));
const bottomSpacer = computed(() => (virtual.value ? (props.count - end.value) * rowHeight.value : 0));

/** 读取真实 scrollTop 并夹到当前可滚动上限，再把结果同时写回 DOM 与 ref。 */
function syncScroll(): void {
  const element = container.value;
  const maxScroll = Math.max(0, props.count * rowHeight.value - viewport.value);
  const current = element ? element.scrollTop : scrollTop.value;
  const next = Math.min(Math.max(0, current), maxScroll);
  if (element && element.scrollTop !== next) element.scrollTop = next;
  scrollTop.value = next;
}
/** scroll 每帧最多处理一次；宿主没有 rAF 时直接同步，保证两种环境下行为一致。 */
let frame = 0;
function onScroll(): void {
  if (frame) return;
  if (typeof requestAnimationFrame !== "function") {
    syncScroll();
    return;
  }
  frame = requestAnimationFrame(() => {
    frame = 0;
    syncScroll();
  });
}
/** 行数或几何变化后重算窗口并夹紧滚动位置：count 变小时必须回收到新的可达范围。 */
watch(() => [props.count, rowHeight.value, viewport.value], syncScroll);
/** 高度实测器：只有真实 clientHeight 才能保证滚动上限与窗口一致。 */
let observer: ResizeObserver | undefined;
/** 挂载后补读一次，再持续实测：浏览器可能恢复了历史滚动位置，首帧就要渲染正确窗口。 */
onMounted(() => {
  syncScroll();
  const element = container.value;
  if (typeof ResizeObserver !== "function" || !element) return;
  observer = new ResizeObserver(entries => {
    const height = entries[0]?.contentRect.height ?? 0;
    // 亚像素抖动不触发重算，避免滚动过程中反复重排。
    if (height > 0 && Math.abs(height - measured.value) >= 1) measured.value = height;
  });
  observer.observe(element);
});
/** 卸载前取消挂起的一帧并断开测量，避免组件销毁后仍写入状态。 */
onBeforeUnmount(() => {
  if (frame) cancelAnimationFrame(frame);
  observer?.disconnect();
});

/** 滚动容器交给宿主：浮层滚动条必须与容器同级渲染，放在内部会跟着内容一起滚。 */
defineExpose({ container });
</script>

<template>
  <div
    ref="container"
    class="virtual-list soft-scrollbar"
    role="list"
    :aria-label="ariaLabel"
    :data-virtual-start="start"
    :data-virtual-end="end"
    :data-virtual-total="count"
    :style="{ height: viewportHeight + 'px' }"
    @scroll="onScroll"
  >
    <!-- 上方占位：把窗口之前的行折叠成一段空白，高度是行高的整数倍。 -->
    <div v-if="virtual" class="virtual-spacer" data-virtual-spacer="top" aria-hidden="true" :style="{ height: topSpacer + 'px' }" />
    <!-- 每一行都是固定高度；posinset/setsize 让读屏用户知道窗口外还有多少行。 -->
    <div
      v-for="index in visible"
      :key="index"
      class="virtual-row"
      role="listitem"
      :aria-posinset="index + 1"
      :aria-setsize="count"
      :style="{ height: itemHeight + 'px' }"
    >
      <slot :index="index" />
    </div>
    <!-- 下方占位：同理补齐剩余行高，否则滚动条会在窗口末尾提前到底。 -->
    <div v-if="virtual" class="virtual-spacer" data-virtual-spacer="bottom" aria-hidden="true" :style="{ height: bottomSpacer + 'px' }" />
  </div>
</template>

<style scoped>
.virtual-list {
  overflow-y: auto;
  overscroll-behavior: contain;
}

/* spacer 只贡献高度，不参与命中与读屏。 */
.virtual-spacer {
  width: 100%;
}

/* 行高由 prop 决定，边框必须算进高度，否则每行多出的 1px 会累积成错位。 */
.virtual-row {
  box-sizing: border-box;
}
</style>
