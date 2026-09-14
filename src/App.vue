<script setup lang="ts">
import NoteToolbar from "./components/NoteToolbar.vue";
import NoteTabs from "./components/NoteTabs.vue";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import AiSplitDialog from "./components/AiSplitDialog.vue";
import CardEditor from "./components/CardEditor.vue";
import CardPanel from "./components/CardPanel.vue";
import CommandPalette from "./components/CommandPalette.vue";
import EditorPane from "./components/EditorPane.vue";
import FileTree from "./components/FileTree.vue";
import QuickCapture from "./components/QuickCapture.vue";
import ReviewWorkspace from "./components/ReviewWorkspace.vue";
import Ribbon from "./components/Ribbon.vue";
import SettingsOverlay from "./components/SettingsOverlay.vue";
import StatusBar from "./components/StatusBar.vue";
import VaultSetup from "./components/VaultSetup.vue";
import AgentWorkspace from "./components/agent/AgentWorkspace.vue";
import AgentDiff from "./components/agent/AgentDiff.vue";
import VideoWorkspace from "./components/video/VideoWorkspace.vue";
import { useAgentFlow } from "./composables/useAgentFlow";
import { deriveFolderNames } from "./components/fileTree/treeModel";
import { useAiSplitFlow } from "./composables/useAiSplitFlow";
import { useCardEditorFlow } from "./composables/useCardEditorFlow";
import { useNoteActions } from "./composables/useNoteActions";
import { useNoteTabs } from "./composables/useNoteTabs";
import { useReviewSessionFlow } from "./composables/useReviewSessionFlow";
import { useToast } from "./composables/useToast";
import { countContentWords } from "./markdown/model";
import { createNoteFile, deleteFolder, deleteNoteFile, clearSelectedNote, initialize, leaveVault, openVault, rescanVault, selectNote } from "./services/appState";
import { activeCardId, activeNoteCards } from "./services/stores/cardStore";
import { activeNoteContent, activeNotePath, extraFolders, findNote, notes, renameNoteFile, renameFolder, createFolder, updateNoteDraft, notePersistence, noteOperationBusy, lastNotePathChange } from "./services/stores/noteStore";
import { remapNotePath } from "./domain/notePaths";
import { resolveError } from "./utils/errorMessage";
import { activeProviderId, providers, setActiveProvider } from "./services/stores/providerStore";
import { closeVideoWorkspace, openVideoWorkspace, videoState } from "./services/stores/videoStore";
import { aiGradingEnabled, setAiGradingEnabled, studyStats } from "./services/stores/reviewStore";
import { applyFontSize, applyTheme, fontSize, initialized, setFontSize, theme, toggleTheme } from "./services/stores/uiStore";
import { attachmentFolderSaving, attachmentFolderStatus, dataLocations, recentVaults, setAttachmentFolder, setVaultPassword, vaultConfig, vaultPath, vaultStatus } from "./services/stores/vaultStore";
import { revealDataFolder } from "./api/backend";

const { toastMessage, showToast } = useToast();

/** 布局开关：文件树与卡片面板的展开状态。 */
const treeOpen = ref(true);
const panelOpen = ref(false);
const paletteOpen = ref(false);
const captureOpen = ref(false);
const settingsOpen = ref(false);

/** 当前视图：初始化完成后按 Vault 是否存在切换。 */
const view = computed(() => {
  if (!initialized.value) {
    return "loading" as const;
  }
  return vaultPath.value === null ? ("vault" as const) : ("main" as const);
});
/** 当前笔记摘要。 */
const activeNote = computed(() => findNote(activeNotePath.value));
/** Vault 显示名。 */
const vaultName = computed(() => {
  const path = vaultPath.value ?? "";
  return path.split(/[\\/]/).filter(Boolean).pop() ?? "我的知识库";
});
/** 完整文件夹列表：由笔记路径与已建空文件夹推导。 */
const folderList = computed(() => deriveFolderNames(notes.value, extraFolders.value));
const wordCount = computed(() => countContentWords(activeNoteContent.value));

const { cardEditor, cardSaving, reselectCardSource, openCardEditor, openCardEditorFromSelection, handleCardEditorSave, editCard, handleDeleteCard } = useCardEditorFlow({ showToast });

/** 改名或移动成功后同步已加载卡片与编辑草稿的所属笔记；同步执行才能跟上连续多次移动。 */
watch(lastNotePathChange, (change) => {
  if (!change) return;
  const remap = (path: string): string => remapNotePath(path, change.oldPath, change.newPath);
  activeNoteCards.value = activeNoteCards.value.map((card) => ({ ...card, notePath: remap(card.notePath) }));
  if (cardEditor.value.notePath) cardEditor.value.notePath = remap(cardEditor.value.notePath);
}, { flush: "sync" });

/** 回传改名结果，输入框只有在后端确认成功后才关闭。 */
async function handleRename(oldPath: string, newPath: string, done: (error?: string) => void, action: typeof renameNoteFile): Promise<void> {
  try { await action(oldPath, newPath); done(); }
  catch (error) { done(resolveError(error)); }
}

const editorPane = ref<InstanceType<typeof EditorPane> | null>(null);
const { aiSplit, providerConfigured, openAiSplit, closeAiSplit, startAiSplit, stopAiSplit, setAiSplitScope, setAiSplitCount, toggleAiDraft, removeAiDraft, handleAiSplitAdopt } = useAiSplitFlow({ showToast, getSelection: () => editorPane.value?.getSelection() ?? null });
const agent = useAgentFlow({ showToast, selectNote, openSplit: openAiSplit });
const agentState = agent.state;
const { reviewSession, startReviewFromNote, startTodayReview } = useReviewSessionFlow({ showToast });
/** 打开视频转笔记：收起其他工作区后再加载设置与组件状态。 */
async function openVideo(): Promise<void> {
  agentState.open = false;
  reviewSession.value.open = false;
  await openVideoWorkspace();
}
/** 工作区互斥展示；隐藏复习时保留已挂载的会话和作答草稿。 */
watch(() => reviewSession.value.open, (open) => { if (open) { agentState.open = false; videoState.open = false; } }, { flush: "sync" });
watch(() => agentState.open, (open) => { if (open) { reviewSession.value.open = false; videoState.open = false; } }, { flush: "sync" });
watch(() => videoState.open, (open) => { if (open) { agentState.open = false; reviewSession.value.open = false; } }, { flush: "sync" });
const { handleSelectNote, handleQuickCapture, handleDeleteSelection, handleMoveEntries } = useNoteActions({
  showToast,
  // 窄窗口下选择笔记后收起抽屉。
  onNoteOpened: () => {
    agentState.open = false;
    reviewSession.value.open = false;
    videoState.open = false;
    if (window.innerWidth < 768) {
      treeOpen.value = false;
    }
  },
});

/** 标签只跟踪实际打开的笔记，保存失败时保留页面而不是丢弃草稿。 */
const { tabs: noteTabs, close: closeNoteTab, closing: tabClosing } = useNoteTabs({
  notes, activePath: activeNotePath, vaultPath, pathChange: lastNotePathChange, busy: noteOperationBusy,
  select: handleSelectNote, flush: (path) => notePersistence.flush(path), clearActive: clearSelectedNote, onError: showToast,
});

/** 快速捕获创建笔记，成功后关闭对话框。 */
async function submitQuickCapture(title: string, folder: string, body: string): Promise<void> {
  if (await handleQuickCapture(title, folder, body)) {
    captureOpen.value = false;
  }
}

/** 从设置中打开最近 Vault：复用开库流程并收起设置层。 */
async function openRecentVault(path: string): Promise<void> {
  settingsOpen.value = false;
  await openVault(path);
}

/** 在系统文件管理器中打开数据目录，失败时以 toast 提示。 */
async function handleRevealDataFolder(target: "cards" | "config"): Promise<void> {
  try {
    await revealDataFolder(target);
  } catch (error) {
    showToast((error as { message?: string })?.message ?? "无法打开文件夹");
  }
}

/** 运行命令面板动作。 */
function runCommand(actionId: string): void {
  if (actionId === "new-note" || actionId === "capture") {
    captureOpen.value = true;
  } else if (actionId === "today-review") {
    startTodayReview();
  } else if (actionId === "toggle-theme") {
    toggleTheme();
  } else if (actionId === "toggle-panel") {
    panelOpen.value = !panelOpen.value;
  } else if (actionId === "settings") {
    settingsOpen.value = true;
  }
}

/** 关闭最上层覆盖层。 */
function closeTopOverlay(): void {
  if (cardEditor.value.open) {
    if (cardSaving.value) return;
    cardEditor.value.open = false;
    return;
  }
  if (aiSplit.value.open) {
    closeAiSplit();
    return;
  }
  if (paletteOpen.value) {
    paletteOpen.value = false;
    return;
  }
  if (captureOpen.value) {
    captureOpen.value = false;
    return;
  }
  if (settingsOpen.value) {
    settingsOpen.value = false;
    return;
  }
  if (videoState.open) {
    closeVideoWorkspace();
  }
}

/** 全局快捷键复用现有入口；Alt+B 仅在无遮挡的笔记工作区切换右栏。 */
function handleGlobalKeydown(event: KeyboardEvent): void {
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    paletteOpen.value = true;
    return;
  }
  if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "n") {
    event.preventDefault();
    captureOpen.value = true;
    return;
  }
  if (event.key === "Escape") {
    closeTopOverlay();
  }
  if (event.altKey && !event.ctrlKey && !event.metaKey && event.key.toLowerCase() === "b" && !agentState.open && !reviewSession.value.open && !videoState.open
    && !settingsOpen.value && !paletteOpen.value && !captureOpen.value && !cardEditor.value.open && !aiSplit.value.open) {
    event.preventDefault();
    panelOpen.value = !panelOpen.value;
  }
}

/** 窗口重新聚焦时扫描 Vault，检测外部修改（5 秒节流）。 */
let lastRescanAt = 0;
function handleWindowFocus(): void {
  const now = Date.now();
  if (now - lastRescanAt < 5000) {
    return;
  }
  lastRescanAt = now;
  void rescanVault();
}

/** 屏蔽浏览器原生右键菜单（应用内所有菜单均为自绘）。 */
function preventNativeContextMenu(event: MouseEvent): void {
  event.preventDefault();
}

onMounted(() => {
  applyTheme();
  applyFontSize();
  void initialize();
  window.addEventListener("keydown", handleGlobalKeydown);
  window.addEventListener("focus", handleWindowFocus);
  window.addEventListener("contextmenu", preventNativeContextMenu);
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", handleGlobalKeydown);
  window.removeEventListener("focus", handleWindowFocus);
  window.removeEventListener("contextmenu", preventNativeContextMenu);
});

watch(() => theme.value, applyTheme);
</script>

<template>
  <!-- 初始化中 -->
  <div v-if="view === 'loading'" class="flex min-h-screen items-center justify-center bg-bg text-[12px] text-ink-3">
    正在加载本地数据…
  </div>

  <!-- 首次启动：选择 Vault -->
  <VaultSetup v-else-if="view === 'vault'" :recents="recentVaults" @open-vault="(path) => void openVault(path)" />

  <!-- 笔记工作台 -->
  <div v-else class="flex h-screen min-h-0 flex-col overflow-hidden">
    <div class="flex min-h-0 flex-1">
      <Ribbon
        :agent-open="agentState.open"
        :review-open="reviewSession.open"
        :video-open="videoState.open"
        @open-agent="agent.open"
        @open-video="openVideo"
        :tree-open="treeOpen"
        :due-count="studyStats.dueCount"
        :dark="theme === 'dark'"
        @toggle-tree="treeOpen = !treeOpen"
        @open-palette="paletteOpen = true"
        @open-capture="captureOpen = true"
        @open-review="startTodayReview"
        @toggle-theme="toggleTheme"
        @open-settings="settingsOpen = true"
      />

      <aside
        class="shrink-0 overflow-hidden border-r border-hairline bg-bg-side transition-[width] duration-200 max-md:fixed max-md:inset-y-0 max-md:left-[52px] max-md:z-40 max-md:shadow-xl"
        :inert="!treeOpen"
        :class="treeOpen ? 'w-[252px]' : 'w-0 border-r-0'"
      >
        <div class="h-full w-[252px]">
          <FileTree
            :notes="notes"
            :folder-names="folderList"
            :active-note-path="activeNotePath"
            :due-count="studyStats.dueCount"
            @select-note="(path) => void handleSelectNote(path)"
            @note-created="(folder, title) => void createNoteFile(folder, title, '')"
            @folder-created="(path) => void createFolder(path)"
            :path-change="lastNotePathChange"
            @rename-note="(oldPath, newPath, done) => void handleRename(oldPath, newPath, done, renameNoteFile)"
            @delete-note="(path) => void deleteNoteFile(path)"
            @rename-folder="(oldPath, newPath, done) => void handleRename(oldPath, newPath, done, renameFolder)"
            @delete-folder="(path) => void deleteFolder(path)"
            @delete-selection="(items) => void handleDeleteSelection(items)"
            @move-entries="(moves) => void handleMoveEntries(moves)"
            @open-review="startTodayReview"
          />
        </div>
      </aside>
      <!-- 窄窗口抽屉遮罩 -->
      <div v-if="treeOpen" class="fixed inset-0 left-[52px] z-30 bg-ink/30 md:hidden" @click="treeOpen = false" />

      <AgentWorkspace v-if="agentState.open"
        :images="agentState.images" :reading-images="agentState.readingImages" @paste-images="agent.pasteImages" @remove-image="agent.removeImage"
        :session="agentState.session" :sessions="agentState.sessions" :run="agentState.run" :draft="agentState.draft"
        :selected-paths="agentState.selectedPaths" :provider-id="activeProviderId" :providers="providers" :notes="notes"
        :memory="agentState.memory" :error="agentState.error" :loading="agentState.loading" :sending="agentState.sending"
        :children="agentState.children" :children-error="agentState.childrenError" :child-detail="agentState.childDetail"
        @open-child="agent.openChild" @close-child="agent.closeChild" @refresh-child="agent.refreshChild"
        @refresh-children="agent.refreshChildren"
        @interrupt-child="agent.interruptChild" @message-child="agent.messageChild" @resume-goal="agent.resumeGoal"
        :ai-grading="aiGradingEnabled" :review-flow="agent.reviewFlow"
        :suspended="settingsOpen || paletteOpen || captureOpen || reviewSession.open || aiSplit.open || Boolean(agentState.change)"
        @send="agent.send" @stop="agent.stop" @new-session="agent.select()" @session="agent.select" @delete-session="agent.deleteSession"
        @close="agentState.open = false" @settings="settingsOpen = true"
        @draft="agentState.draft = $event" @scope="agentState.selectedPaths = $event"
        @action="agent.action" @memory="agent.saveMemory" @adopt="agent.adopt" />
      <ReviewWorkspace
        v-if="reviewSession.id" v-show="reviewSession.open" :key="reviewSession.id"
        :title="reviewSession.title" :note-path="reviewSession.notePath" :include-all="reviewSession.includeAll"
        :suspended="!reviewSession.open || settingsOpen || paletteOpen || captureOpen || aiSplit.open || cardEditor.open || Boolean(agentState.change)"
        @close="reviewSession.open = false"
      />
      <VideoWorkspace v-if="videoState.open" @close="closeVideoWorkspace" @open-note="(path: string) => void handleSelectNote(path)" />
      <main v-if="!agentState.open && !reviewSession.open && !videoState.open" class="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-bg-paper">
        <EditorPane
          v-if="activeNote"
          ref="editorPane"
          :note-path="activeNote.path"
          :tabs="noteTabs" :tabs-busy="tabClosing || noteOperationBusy"
          @select-tab="(path) => void handleSelectNote(path)" @close-tab="(path) => void closeNoteTab(path)"
          :panel-open="panelOpen"
          @toggle-panel="panelOpen = !panelOpen"
          :content="activeNoteContent"
          :dark="theme === 'dark'"
          :read-only="noteOperationBusy"
          :cards="activeNoteCards"
          @card-click="activeCardId = $event"
          @create-card="openCardEditorFromSelection"
          @save-content="updateNoteDraft"
        />
        <template v-else>
          <NoteTabs v-if="noteTabs.length" :tabs="noteTabs" :active-path="activeNotePath ?? ''" :busy="tabClosing || noteOperationBusy" @select="(path) => void handleSelectNote(path)" @close="(path) => void closeNoteTab(path)">
            <template #actions><NoteToolbar inline :panel-open="panelOpen" @toggle-panel="panelOpen = !panelOpen" /></template>
          </NoteTabs>
          <NoteToolbar v-else :panel-open="panelOpen" @toggle-panel="panelOpen = !panelOpen" />
          <div class="flex min-h-0 flex-1 items-center justify-center text-[13px] text-ink-3">选择或创建一篇笔记</div>
        </template>
      </main>

      <aside
        class="shrink-0 overflow-hidden border-l border-hairline bg-bg-side transition-[width] duration-200 max-lg:fixed max-lg:inset-y-0 max-lg:right-0 max-lg:z-40 max-lg:shadow-xl"
        id="note-card-panel"
        :inert="!panelOpen"
        v-show="!agentState.open && !reviewSession.open && !videoState.open"
        :class="panelOpen ? 'w-[300px]' : 'w-0 border-l-0'"
      >
        <div class="h-full w-[300px]">
          <CardPanel
            v-if="activeNote"
            :note-path="activeNote.path"
            :cards="activeNoteCards"
            :content="activeNoteContent"
            :active-card-id="activeCardId"
            @edit-card="editCard"
            @delete-card="(id) => void handleDeleteCard(id)"
            @add-card="openCardEditor(activeNoteCards[0]?.kind ?? 'qa')"
            @open-ai-split="openAiSplit"
            @start-review="startReviewFromNote"
            @collapse="panelOpen = false"
          />
        </div>
      </aside>
      <!-- 中窄窗口右栏改为抽屉，避免两侧栏同时挤压正文。 -->
      <div v-if="panelOpen && !agentState.open && !reviewSession.open && !videoState.open" class="fixed inset-0 z-30 bg-ink/30 lg:hidden" @click="panelOpen = false" />
    </div>

    <StatusBar
      :vault-name="vaultName"
      :word-count="wordCount"
      :due-count="studyStats.dueCount"
      :note-title="activeNote?.title ?? ''"
      :dark="theme === 'dark'"
      @toggle-theme="toggleTheme"
    />

    <!-- 覆盖层 -->
    <AgentDiff v-if="agentState.change" :change="agentState.change" :busy="agentState.run?.state === 'running' || noteOperationBusy"
      @close="agentState.change = null" @undo="agent.undo" @note="(path) => { agentState.change = null; void agent.action('note', path); }" />
    <CommandPalette
      v-if="paletteOpen"
      :notes="notes"
      @close="paletteOpen = false"
      @select-note="(path) => { void handleSelectNote(path); paletteOpen = false; }"
      @select-card="(notePath, cardId) => { void handleSelectNote(notePath); activeCardId = cardId; paletteOpen = false; }"
      @run-action="runCommand"
    />

    <QuickCapture
      v-if="captureOpen"
      :folders="folderList"
      @close="captureOpen = false"
      @create="(title, folder, body) => void submitQuickCapture(title, folder, body)"
    />

    <SettingsOverlay
      v-if="settingsOpen"
      :theme="theme"
      :font-size="fontSize"
      :vault-path="vaultPath ?? ''"
      :recent-vaults="recentVaults"
      :providers="providers"
      :active-provider-id="activeProviderId"
      :vault-status="vaultStatus"
      :ai-grading-enabled="aiGradingEnabled"
      :attachment-folder="vaultConfig.attachmentFolder"
      :attachment-folder-saving="attachmentFolderSaving"
      :attachment-folder-status="attachmentFolderStatus"
      :data-locations="dataLocations"
      @close="settingsOpen = false"
      @update-theme="theme = $event"
      @update-font-size="(size) => void setFontSize(size)"
      @change-vault="leaveVault"
      @open-recent-vault="(path) => void openRecentVault(path)"
      @set-active-provider="(id) => void setActiveProvider(id)"
      @set-vault-password="(password) => void setVaultPassword(password)"
      @update-ai-grading="(enabled) => void setAiGradingEnabled(enabled)"
      @save-attachment-folder="(folder) => void setAttachmentFolder(folder)"
      @reveal-data-folder="(target) => void handleRevealDataFolder(target)"
    />

    <AiSplitDialog
      v-if="aiSplit.open"
      :state="aiSplit"
      :provider-configured="providerConfigured"
      @close="closeAiSplit"
      @open-settings="() => { closeAiSplit(); settingsOpen = true; }"
      @start="startAiSplit"
      @stop="stopAiSplit"
      @scope="setAiSplitScope"
      @count="setAiSplitCount"
      @toggle="toggleAiDraft"
      @remove="removeAiDraft"
      @adopt="handleAiSplitAdopt"
    />

    <CardEditor
      v-if="cardEditor.open"
      :kind="cardEditor.kind"
      :editing-id="cardEditor.editingId"
      :front="cardEditor.front"
      :back="cardEditor.back"
      :detail="cardEditor.detail"
      :example="cardEditor.example"
      :rubric="cardEditor.rubric"
      :busy="cardSaving"
      :has-source="Boolean(cardEditor.source)"
      @reselect-source="reselectCardSource"
      @close="cardEditor.open = false"
      @save="(draft) => void handleCardEditorSave(draft)"
    />

    <Transition name="toast">
      <div
        v-if="toastMessage"
        class="fixed bottom-9 left-1/2 z-90 -translate-x-1/2 rounded-lg bg-ink px-3.5 py-2 text-[12px] font-medium text-bg shadow-lg"
      >
        {{ toastMessage }}
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.toast-enter-active,
.toast-leave-active {
  transition: opacity 150ms, transform 150ms;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translate(-50%, 6px);
}
</style>
