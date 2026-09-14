<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch, type CSSProperties } from "vue";

/**
 * 虚拟滚动条：用浮层 thumb 表达滚动位置，thumb 本身是拖拽把手，做法对齐 Element Plus 的 el-scrollbar。
 *
 * 为什么不用原生滚动条：Windows/WebView2 的原生条带两端箭头、还要占掉一条高度；而且只要写了
 * scrollbar-width/scrollbar-color，Chromium 就退出自定义模式，::-webkit-scrollbar 会被整段忽略。
 * 所以滚动容器统一用 .soft-scrollbar 隐藏原生条，位置由本组件自己画。
 *
 * 契约：
 * 1. 本组件必须与 target 同级渲染（同一个父元素）。浮层按 target 的 offset* 绝对定位，两者共享同一个
 *    定位祖先，坐标才对得上；放进 target 内部则会跟着内容一起滚。
 * 2. 这个共同父元素必须自身是定位元素（relative 等）。否则浮层的包含块会一路退到 body：既躲开宿主
 *    overflow-hidden 的裁剪，容器落到视口外时还会凭空给文档长出一条原生横向滚动条。
 * 3. 只读写 target 的几何与 scrollLeft/scrollTop，不劫持滚轮、键盘与点击：可交互的只有贴边窄带与 thumb。
 * 4. 不溢出时不渲染；thumb 会在悬停、滚动后的一小段时间内显示。
 */
const props = withDefaults(defineProps<{
  /** 受控滚动容器；父组件模板 ref 在首次渲染后才有值，因此绑定要在挂载后再补一次。 */
  target: HTMLElement | null;
  /** 纵轴用于普通窗格，横轴用于标签条这类横向容器。 */
  orientation?: "vertical" | "horizontal";
}>(), { orientation: "vertical" });

/** 轴向差异集中在这张表里：几何、位移、指针坐标都按它取，避免散落的 if (vertical)。 */
const AXES = {
  vertical: { viewport: "clientHeight", content: "scrollHeight", scroll: "scrollTop", shift: "translateY", pointer: "clientY", size: "height", start: "top" },
  horizontal: { viewport: "clientWidth", content: "scrollWidth", scroll: "scrollLeft", shift: "translateX", pointer: "clientX", size: "width", start: "left" },
} as const;

/** thumb 最短长度：内容远多于视口时也要留得住抓得住的把手（相当于 el-scrollbar 的 minSize）。 */
const MIN_THUMB = 28;
/** 滚动结束后继续显示的时长：滚轮滚完 thumb 不该立刻消失，指针移出容器也不至于闪没。 */
const LINGER_MS = 900;

const axis = computed(() => AXES[props.orientation]);
const root = ref<HTMLElement | null>(null);
const box = ref({ left: 0, top: 0, width: 0, height: 0 });
const viewportLength = ref(0);
const contentLength = ref(0);
const scrollOffset = ref(0);
const hovering = ref(false);
const lingering = ref(false);
const dragging = ref(false);

const maxScroll = computed(() => Math.max(0, contentLength.value - viewportLength.value));
const overflowing = computed(() => maxScroll.value > 0);
const visible = computed(() => overflowing.value && (hovering.value || lingering.value || dragging.value));
/** thumb 长度取视口占内容的比例；位移按剩余行程等比换算，被 minSize 夹住时也不会拖过头。 */
const thumbLength = computed(() => overflowing.value
  ? Math.round(Math.min(viewportLength.value, Math.max(MIN_THUMB, (viewportLength.value ** 2) / contentLength.value)))
  : 0);
const thumbOffset = computed(() => {
  const travel = viewportLength.value - thumbLength.value;
  return travel <= 0 ? 0 : Math.round((scrollOffset.value / maxScroll.value) * travel);
});
const boxStyle = computed<CSSProperties>(() => ({ left: `${box.value.left}px`, top: `${box.value.top}px`, width: `${box.value.width}px`, height: `${box.value.height}px` }));
const thumbStyle = computed<CSSProperties>(() => ({ [axis.value.size]: `${thumbLength.value}px`, transform: `${axis.value.shift}(${thumbOffset.value}px)` }) as CSSProperties);

/** 把长度夹进 [0, limit]：容器被裁到视口外时，浮层不能跟着跑到视口外。 */
function clamp(value: number, limit: number): number {
  return Math.max(0, Math.min(value, limit));
}

/**
 * 浮层对齐 target 的盒子：offset* 与绝对定位都相对同一个定位祖先，因此可以直接照搬。
 * 再夹一次视口：万一宿主漏了定位祖先（包含块退到 body、失去裁剪），浮层也不会把文档撑宽出
 * 一条原生横向滚动条——最坏只是被裁短，不会污染整个窗口。
 */
function measureBox(): void {
  const el = props.target;
  if (!el) { box.value = { left: 0, top: 0, width: 0, height: 0 }; return; }
  const left = clamp(el.offsetLeft, window.innerWidth);
  const top = clamp(el.offsetTop, window.innerHeight);
  box.value = {
    left, top,
    width: clamp(el.offsetWidth, window.innerWidth - left),
    height: clamp(el.offsetHeight, window.innerHeight - top),
  };
}

/** 读取容器几何：滚动事件本身已按帧合并，直接测量不会带来额外布局开销。 */
function measure(): void {
  const el = props.target;
  viewportLength.value = el ? el[axis.value.viewport] : 0;
  contentLength.value = el ? el[axis.value.content] : 0;
  scrollOffset.value = el ? el[axis.value.scroll] : 0;
  measureBox();
}

let lingerTimer = 0;
/** 滚动或拖拽后短暂保留 thumb，让"刚刚滚过"有可见的落点。 */
function linger(): void {
  lingering.value = true;
  window.clearTimeout(lingerTimer);
  lingerTimer = window.setTimeout(() => { lingering.value = false; }, LINGER_MS);
}

function onScroll(): void {
  measure();
  if (overflowing.value) linger();
}

function onPointerEnter(): void { hovering.value = true; }
function onPointerLeave(): void { hovering.value = false; }

let resizeObserver: ResizeObserver | null = null;
let mutationObserver: MutationObserver | null = null;
let boundTarget: HTMLElement | null = null;

/** 容器在 flex 里尺寸固定、自身不触发 resize，所以连内容与父级一起观察：父级变宽会挪动容器位置。 */
function observeSizes(): void {
  const observer = resizeObserver;
  if (!observer || !boundTarget) return;
  observer.disconnect();
  observer.observe(boundTarget);
  if (boundTarget.parentElement) observer.observe(boundTarget.parentElement);
  Array.from(boundTarget.children).forEach((child) => observer.observe(child));
}

/** 解绑旧容器：target 变化或组件卸载时，监听与观察者都不能留在上一个容器上。 */
function unbind(): void {
  boundTarget?.removeEventListener("scroll", onScroll);
  boundTarget?.removeEventListener("pointerenter", onPointerEnter);
  boundTarget?.removeEventListener("pointerleave", onPointerLeave);
  boundTarget = null;
  resizeObserver?.disconnect();
  mutationObserver?.disconnect();
  window.removeEventListener("resize", measure);
}

/** 绑定容器：scroll 同步位置，ResizeObserver 覆盖尺寸与位置变化，MutationObserver 覆盖内容增删。 */
function bind(): void {
  const el = props.target;
  if (el === boundTarget) return;
  unbind();
  if (!el) return;
  boundTarget = el;
  el.addEventListener("scroll", onScroll, { passive: true });
  el.addEventListener("pointerenter", onPointerEnter);
  el.addEventListener("pointerleave", onPointerLeave);
  // jsdom 不提供 ResizeObserver，测试里几何由 scroll 事件驱动；生产环境（WebView2）始终可用。
  if (typeof ResizeObserver !== "undefined") resizeObserver = new ResizeObserver(measure);
  if (typeof MutationObserver !== "undefined") {
    mutationObserver = new MutationObserver(() => { measure(); observeSizes(); });
    mutationObserver.observe(el, { childList: true, subtree: true, characterData: true });
  }
  window.addEventListener("resize", measure);
  observeSizes();
  measure();
}

/** 拖拽 thumb：位移按"内容长度 / 剩余行程"放大成滚动量，写回滚动位置后由 scroll 事件回写几何。 */
function beginDrag(event: PointerEvent, container: HTMLElement, startOffset: number): void {
  const handle = event.currentTarget as HTMLElement;
  const travel = viewportLength.value - thumbLength.value;
  const ratio = travel > 0 ? maxScroll.value / travel : 1;
  const start = event[axis.value.pointer];
  const scrollKey = axis.value.scroll;
  dragging.value = true;
  // 拖拽期间按住的是浮层，浏览器仍可能顺手选中正文，这里按 el-scrollbar 的做法清掉选区。
  window.getSelection()?.removeAllRanges();
  // 指针捕获让 move/up 始终回到 thumb 上，指针拖出容器也不会丢失拖拽。
  handle.setPointerCapture(event.pointerId);
  function onPointerMove(moveEvent: PointerEvent): void {
    container[scrollKey] = startOffset + (moveEvent[axis.value.pointer] - start) * ratio;
    measure();
  }
  function onPointerUp(): void {
    dragging.value = false;
    handle.removeEventListener("pointermove", onPointerMove);
    handle.removeEventListener("pointerup", onPointerUp);
    handle.removeEventListener("pointercancel", onPointerUp);
    linger();
  }
  handle.addEventListener("pointermove", onPointerMove);
  handle.addEventListener("pointerup", onPointerUp);
  handle.addEventListener("pointercancel", onPointerUp);
}

/** 按住 thumb：从当前位置起拖，抓在哪里都不会跳。 */
function startDrag(event: PointerEvent): void {
  const el = props.target;
  if (!el || event.button !== 0 || !overflowing.value) return;
  event.preventDefault();
  beginDrag(event, el, el[axis.value.scroll]);
}

/** 点贴边窄带：先把 thumb 中心对到指针处，再顺势进入拖拽，等于 el-scrollbar 的 clickTrackHandler + 继续拖动。 */
function jumpTo(event: PointerEvent): void {
  const el = props.target;
  if (!el || event.button !== 0 || !overflowing.value) return;
  event.preventDefault();
  const travel = viewportLength.value - thumbLength.value;
  if (travel <= 0) return;
  const edge = axis.value.start === "top" ? "top" : "left";
  const origin = (event.currentTarget as HTMLElement).getBoundingClientRect()[edge];
  const centered = event[axis.value.pointer] - origin - thumbLength.value / 2;
  el[axis.value.scroll] = (Math.min(Math.max(centered, 0), travel) / travel) * maxScroll.value;
  beginDrag(event, el, el[axis.value.scroll]);
}

watch(() => props.target, bind, { flush: "post", immediate: true });
/** 模板 ref 在父组件挂载过程中才落到 DOM 上，首次渲染拿到的 target 可能是 null，挂载后补一次绑定。 */
onMounted(() => { bind(); void nextTick(bind); });
onBeforeUnmount(() => { window.clearTimeout(lingerTimer); unbind(); });
</script>

<template>
  <div v-if="overflowing" ref="root" class="virtual-scrollbar" :class="[`is-${orientation}`, { 'is-visible': visible, 'is-dragging': dragging }]" :style="boxStyle" aria-hidden="true" data-virtual-scrollbar :data-orientation="orientation" :data-scroll-max="maxScroll" :data-thumb-length="thumbLength" :data-thumb-offset="thumbOffset">
    <div class="virtual-track" @pointerdown="jumpTo" @pointerenter="onPointerEnter" @pointerleave="onPointerLeave">
      <div class="virtual-thumb" :style="thumbStyle" @pointerdown.stop="startDrag">
        <span class="virtual-thumb-bar" />
      </div>
    </div>
  </div>
</template>

<style scoped>
/* 浮层只负责定位与命中；命中区只有贴边窄带，窗格其余区域的点击照常落到内容上。 */
/* margin 清零：宿主可能用 space-y-* 给兄弟节点加间距，绝对定位元素同样会被它顶偏。 */
.virtual-scrollbar { position: absolute; margin: 0; pointer-events: none; opacity: 0; transition: opacity 120ms ease-out; }
.virtual-scrollbar.is-visible { opacity: 1; transition-duration: 260ms; }
/* 窄带 5px：够点中做跳转，又不会把窗格边缘变成点击陷阱。 */
.virtual-track { position: absolute; pointer-events: auto; }
.virtual-scrollbar.is-horizontal .virtual-track { right: 0; bottom: 0; left: 0; height: 5px; }
.virtual-scrollbar.is-vertical .virtual-track { top: 0; right: 0; bottom: 0; width: 5px; }
/* thumb 自己再往容器内侧加高命中区，方便抓住。 */
.virtual-thumb { position: absolute; cursor: grab; touch-action: none; }
.virtual-scrollbar.is-horizontal .virtual-thumb { bottom: 0; left: 0; height: 12px; }
.virtual-scrollbar.is-vertical .virtual-thumb { top: 0; right: 0; width: 12px; }
.virtual-scrollbar.is-dragging .virtual-thumb { cursor: grabbing; }
.virtual-thumb-bar { position: absolute; border-radius: 999px; background: color-mix(in srgb, var(--qc-ink-3) 45%, transparent); }
.virtual-scrollbar.is-horizontal .virtual-thumb-bar { right: 0; bottom: 1px; left: 0; height: 4px; }
.virtual-scrollbar.is-vertical .virtual-thumb-bar { top: 0; right: 1px; bottom: 0; width: 4px; }
.virtual-thumb:hover .virtual-thumb-bar,
.virtual-scrollbar.is-dragging .virtual-thumb-bar { background: color-mix(in srgb, var(--qc-ink-3) 75%, transparent); }
</style>
