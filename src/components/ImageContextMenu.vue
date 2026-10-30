<script setup lang="ts">
import { onBeforeUnmount, onMounted } from "vue";
import type { ImageMenuRequest } from "../domain/imageMenu";

/**
 * 图片右键菜单：纯展示组件，按请求坐标摆放三项动作，只向上抛意图。
 *
 * 契约：
 * - 不调用后端、不持有业务状态（busy 由用例层注入），因此可脱离壳层单独测试；
 * - 坐标已由 domain/imageMenu 的 clampImageMenuPosition 钳制进视口，这里直接使用；
 * - 不做标题行、不配图标，行高字号与文件树右键菜单逐字一致：应用内所有右键菜单
 *   必须像同一个菜单，带标题或大图标的浮层会被读成弹窗而不是菜单。
 */
defineProps<{ request: ImageMenuRequest; busy: boolean }>();

const emit = defineEmits<{
  copy: [];
  save: [];
  reveal: [];
  close: [];
}>();

/** 菜单项样式：与 FileTreeMenu.vue 保持同一串类名，只去掉图标留下的间距。 */
const MENU_ITEM = "flex w-full items-center rounded-md px-2.5 py-1.5 text-left text-[12px] text-ink-2 hover:bg-bg-hover disabled:cursor-not-allowed disabled:opacity-50";

/**
 * Esc 关闭菜单。
 *
 * 原因：右键菜单是模态浮层，键盘用户必须有退出路径；面板本身不接收焦点，
 * 所以监听挂在 window 上而不是元素上。
 */
function handleKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    emit("close");
  }
}

/** 挂载后注册全局键监听：Teleport 后菜单已不在原组件子树内，只能全局监听。 */
onMounted(() => window.addEventListener("keydown", handleKeydown));

/** 卸载时移除监听：菜单关闭即销毁，遗留监听会泄漏并误关后续浮层。 */
onBeforeUnmount(() => window.removeEventListener("keydown", handleKeydown));
</script>

<template>
  <Teleport to="body">
    <div
      class="modal-panel fixed z-80 w-[150px] p-1"
      role="menu"
      aria-label="图片操作"
      :style="{ left: `${request.x}px`, top: `${request.y}px` }"
    >
      <!-- 文案用桌面惯用语：本地文件没有"下载"，定位文件叫"在文件夹中显示"。 -->
      <button type="button" role="menuitem" :class="MENU_ITEM" :disabled="busy" @click="emit('copy')">复制图片</button>
      <button type="button" role="menuitem" :class="MENU_ITEM" :disabled="busy" @click="emit('save')">图片另存为…</button>
      <button type="button" role="menuitem" :class="MENU_ITEM" :disabled="busy" @click="emit('reveal')">在文件夹中显示</button>
    </div>
    <!-- 全屏透明遮罩负责点外部关闭：面板 z-80 高于遮罩 z-70，面板内点击不会被拦截。 -->
    <div class="fixed inset-0 z-70" @mousedown="emit('close')" @contextmenu.prevent="emit('close')" />
  </Teleport>
</template>
