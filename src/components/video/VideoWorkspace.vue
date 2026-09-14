<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { LogIn, LogOut, Settings2, UserRound, X } from "@lucide/vue";
import { useVideoFlow } from "../../composables/useVideoFlow";
import BilibiliLoginDialog from "./BilibiliLoginDialog.vue";
import ComponentPanel from "./ComponentPanel.vue";
import VideoLinkForm from "./VideoLinkForm.vue";
import VideoResult from "./VideoResult.vue";
import VideoTaskProgress from "./VideoTaskProgress.vue";
import VideoHistoryPanel from "./VideoHistoryPanel.vue";
import VirtualScrollbar from "../VirtualScrollbar.vue";

/** 正文列滚动容器：浮层滚动条与它同级。 */
const bodyScroll = ref<HTMLElement | null>(null);

const flow = useVideoFlow();
const emit = defineEmits<{ close: []; "open-note": [path: string] }>();

/** 任务存在即展示进度区。 */
const task = computed(() => flow.state.task);
/** 终态才展示结果区，运行中不暴露未完成的路径。 */
const finished = computed(() => Boolean(task.value) && task.value?.state !== "running");
/** 登录只信任同一控制器，旧解析快照不再覆盖退出结果。 */
const loggedIn = computed(() => Boolean(flow.state.login?.loggedIn));
/** 账号菜单只在点击后展开，退出登录立即收起。 */
const userMenuOpen = ref(false);
/** 没有昵称时使用固定文案，避免出现空按钮。 */
const displayName = computed(() => flow.state.login?.name?.trim() || "已登录");
/** 退出登录先收起浮层，再交给登录控制器处理凭据。 */
async function signOut(): Promise<void> {
  userMenuOpen.value = false;
  await flow.logout();
}
watch(loggedIn, value => { if (!value) userMenuOpen.value = false; });
</script>

<template>
  <section class="relative flex h-full min-h-0 min-w-0 flex-1 flex-col bg-bg" aria-label="视频转笔记工作区">
    <header class="flex h-14 shrink-0 items-center gap-1 px-3 sm:gap-2 sm:px-5">
      <span class="shrink-0 text-[15px] font-medium">视频转笔记</span>
      <span class="min-w-0 flex-1 truncate px-2 text-[12px] text-ink-2">
        {{ flow.state.probe?.title ?? "粘贴 B 站链接，生成结构化笔记或字幕稿" }}
      </span>
      <div v-if="loggedIn" class="relative">
        <button
          class="flex items-center gap-2 rounded-full border border-hairline py-1 pl-1 pr-2 text-[12px] hover:bg-bg-hover"
          :aria-expanded="userMenuOpen"
          aria-label="B 站账号"
          @click="userMenuOpen = !userMenuOpen"
        >
          <img v-if="flow.state.avatar" :src="flow.state.avatar" alt="" class="size-6 rounded-full object-cover" />
          <UserRound v-else :size="18" :stroke-width="1.8" />
          <span class="max-w-[96px] truncate">{{ displayName }}</span>
        </button>
        <button v-if="userMenuOpen" class="fixed inset-0 z-20 cursor-default" aria-label="关闭账号菜单" @click="userMenuOpen = false" />
        <div v-if="userMenuOpen" class="absolute right-0 top-full z-30 mt-2 w-44 rounded-xl border border-hairline bg-bg-paper p-3 shadow-lg">
          <div class="flex items-center gap-2">
            <img v-if="flow.state.avatar" :src="flow.state.avatar" alt="" class="size-9 rounded-full object-cover" />
            <span v-else class="grid size-9 place-items-center rounded-full bg-bg-side"><UserRound :size="18" /></span>
            <span class="min-w-0 flex-1 truncate text-[13px]" :title="displayName">{{ displayName }}</span>
          </div>
          <button class="ghost-btn mt-3 w-full justify-center" @click="signOut">
            <LogOut :size="14" :stroke-width="1.8" />退出登录
          </button>
        </div>
      </div>
      <button v-else class="icon-btn" title="扫码登录 B 站" aria-label="扫码登录 B 站" @click="flow.login()">
        <LogIn :size="17" :stroke-width="1.8" />
      </button>
      <button class="icon-btn" title="组件与设置" aria-label="组件与设置" :aria-expanded="flow.state.settingsOpen" @click="flow.toggleSettings()">
        <Settings2 :size="17" :stroke-width="1.8" />
      </button>
      <button class="icon-btn" title="返回笔记" aria-label="返回笔记" @click="emit('close')"><X :size="18" /></button>
    </header>

    <div ref="bodyScroll" class="soft-scrollbar min-h-0 flex-1 overflow-y-auto px-4 pb-12">
      <div class="mx-auto w-full max-w-[780px] space-y-4 pt-2">
        <p v-if="flow.state.error" class="rounded-xl border border-hairline bg-bg-side px-3 py-2 text-[13px] text-danger" role="alert">
          {{ flow.state.error }}
        </p>

        <ComponentPanel
          v-if="flow.state.settingsOpen"
          :settings="flow.state.settings"
          :components="flow.state.components"
          :models="flow.state.models"
          :downloading="flow.state.downloadingModel"
          :download-status="flow.state.downloadStatus"
          @cancel-download="flow.cancelModel"
          @save="flow.saveSettings"
          @download="flow.downloadModel"
          @refresh="flow.refreshComponents"
        />

        <VideoLinkForm
          :url="flow.state.url"
          :probe="flow.state.probe"
          :quality="flow.state.quality"
          :pages="flow.state.pages"
          :screenshots="flow.state.screenshots"
          :logged-in="loggedIn"
          :force-transcribe="flow.state.forceTranscribe"
          @update:force-transcribe="flow.state.forceTranscribe = $event"
          :mode="flow.state.mode"
          @update:mode="flow.state.mode = $event"
          :probing="flow.state.probing"
          :starting="flow.state.starting"
          :running="task?.state === 'running'"
          @update:url="flow.state.url = $event"
          @probe="flow.probe()"
          @update:quality="flow.state.quality = $event"
          @toggle-page="flow.togglePage"
          @update:screenshots="flow.state.screenshots = $event"
          @start="flow.start"
        />

        <p v-if="flow.state.taskError" class="text-[12px] text-ink-2" role="status">进度暂时无法更新，正在自动重试：{{ flow.state.taskError }}</p>
        <VideoTaskProgress v-if="task" :task="task" @stop="flow.stop" />
        <VideoHistoryPanel
          :items="flow.state.history"
          :error="flow.state.historyError"
          :busy="flow.state.probing || flow.state.starting"
          @refresh="flow.refreshHistory"
          @restore="flow.restoreHistory"
          @open-note="(path: string) => emit('open-note', path)"
        />
        <VideoResult
          v-if="task && finished"
          :task="task"
          @open-note="(path: string) => emit('open-note', path)"
          @restart="flow.retry"
        />

        <p v-if="!task" class="px-1 text-[12px] leading-6 text-ink-3">
          提示：有字幕的视频直接读取 B 站字幕（登录后可取 AI 字幕）；没有字幕时用本地 Whisper 转写。
          截图默认开启，可在设置中关闭或调整清晰度。
        </p>
      </div>
    </div>
    <VirtualScrollbar :target="bodyScroll" />

    <BilibiliLoginDialog v-if="flow.state.loginOpen" :login="flow.state.login" @close="flow.closeLogin" @retry="flow.login" />
  </section>
</template>
