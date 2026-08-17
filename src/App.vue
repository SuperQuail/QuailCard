<script setup lang="ts">
import { PanelRightOpen } from "@lucide/vue";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import AiSplitDialog from "./components/AiSplitDialog.vue";
import CardEditor from "./components/CardEditor.vue";
import CardPanel from "./components/CardPanel.vue";
import CommandPalette from "./components/CommandPalette.vue";
import EditorPane from "./components/EditorPane.vue";
import FileTree from "./components/FileTree.vue";
import QuickCapture from "./components/QuickCapture.vue";
import ReviewOverlay from "./components/ReviewOverlay.vue";
import Ribbon from "./components/Ribbon.vue";
import SettingsOverlay from "./components/SettingsOverlay.vue";
import StatusBar from "./components/StatusBar.vue";
import VaultSetup from "./components/VaultSetup.vue";
import { deriveFolderNames } from "./components/fileTree/treeModel";
import { useAiSplitFlow } from "./composables/useAiSplitFlow";
import { useCardEditorFlow } from "./composables/useCardEditorFlow";
import { useNoteActions } from "./composables/useNoteActions";
import { useReviewSessionFlow } from "./composables/useReviewSessionFlow";
import { useToast } from "./composables/useToast";
import { countContentWords, parseMarkdown } from "./markdown";
import { createNoteFile, deleteFolder, deleteNoteFile, initialize, leaveVault, openVault, rescanVault, selectNote } from "./services/appState";
import { activeCardId, activeNoteCards } from "./services/stores/cardStore";
import { activeNoteContent, activeNotePath, extraFolders, findNote, notes, renameNoteFile, renameFolder, saveNoteContent, createFolder } from "./services/stores/noteStore";
import { activeProviderId, providers, setActiveProvider } from "./services/stores/providerStore";
import { aiGradingEnabled, setAiGradingEnabled, studyStats } from "./services/stores/reviewStore";
import { applyFontSize, applyTheme, fontSize, initialized, setFontSize, theme, toggleTheme } from "./services/stores/uiStore";
import { attachmentFolderSaving, attachmentFolderStatus, recentVaults, setAttachmentFolder, setVaultPassword, vaultConfig, vaultPath, vaultStatus } from "./services/stores/vaultStore";

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
/** 当前笔记正文的结构化块。 */
const blocks = computed(() => parseMarkdown(activeNoteContent.value));
/** Vault 显示名。 */
const vaultName = computed(() => {
  const path = vaultPath.value ?? "";
  return path.split(/[\\/]/).filter(Boolean).pop() ?? "我的知识库";
});
/** 完整文件夹列表：由笔记路径与已建空文件夹推导。 */
const folderList = computed(() => deriveFolderNames(notes.value, extraFolders.value));
const wordCount = computed(() => countContentWords(activeNoteContent.value));

const { cardEditor, openCardEditor, openCardEditorFromSelection, handleCardEditorSave, editCard, handleDeleteCard } = useCardEditorFlow({ blocks, showToast });
const { aiSplit, openAiSplit, handleAiSplitAdopt } = useAiSplitFlow({ showToast });
const { reviewSession, startReviewFromNote, startTodayReview } = useReviewSessionFlow({ showToast });
const { handleSelectNote, handleQuickCapture, handleDeleteSelection } = useNoteActions({
  showToast,
  // 窄窗口下选择笔记后收起抽屉。
  onNoteOpened: () => {
    if (window.innerWidth < 768) {
      treeOpen.value = false;
    }
  },
});

/** 快速捕获创建笔记，成功后关闭对话框。 */
async function submitQuickCapture(title: string, folder: string, body: string): Promise<void> {
  if (await handleQuickCapture(title, folder, body)) {
    captureOpen.value = false;
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
  if (reviewSession.value.open) {
    return;
  }
  if (cardEditor.value.open) {
    cardEditor.value.open = false;
    return;
  }
  if (aiSplit.value.open) {
    aiSplit.value.open = false;
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
  }
}

/** 全局快捷键：Ctrl+K 面板、Ctrl+N 捕获、Esc 逐层关闭。 */
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
        class="shrink-0 overflow-hidden border-r border-hairline bg-bg-side transition-[width] duration-200 max-md:fixed max-md:inset-y-0 max-md:left-0 max-md:z-40 max-md:shadow-xl"
        :class="treeOpen ? 'w-[248px]' : 'w-0 border-r-0'"
      >
        <div class="h-full w-[248px]">
          <FileTree
            :notes="notes"
            :folder-names="folderList"
            :active-note-path="activeNotePath"
            :due-count="studyStats.dueCount"
            @select-note="(path) => void handleSelectNote(path)"
            @note-created="(folder, title) => void createNoteFile(folder, title, '')"
            @folder-created="(path) => void createFolder(path)"
            @rename-note="(oldPath, newPath) => void renameNoteFile(oldPath, newPath)"
            @delete-note="(path) => void deleteNoteFile(path)"
            @rename-folder="(oldPath, newPath) => void renameFolder(oldPath, newPath)"
            @delete-folder="(path) => void deleteFolder(path)"
            @delete-selection="(items) => void handleDeleteSelection(items)"
            @open-review="startTodayReview"
          />
        </div>
      </aside>
      <!-- 窄窗口抽屉遮罩 -->
      <div v-if="treeOpen" class="fixed inset-0 z-30 bg-ink/30 md:hidden" @click="treeOpen = false" />

      <main class="soft-scrollbar relative min-w-0 flex-1 overflow-y-auto bg-bg-paper">
        <!-- 卡片面板收起后的重新展开入口（文件树由丝带常驻开关负责） -->
        <button v-if="!panelOpen" type="button" class="icon-btn absolute top-2 right-2 z-30 border border-hairline bg-bg-paper" title="展开卡片面板" @click="panelOpen = true">
          <PanelRightOpen :size="15" />
        </button>
        <EditorPane
          v-if="activeNote"
          :note-path="activeNote.path"
          :content="activeNoteContent"
          :dark="theme === 'dark'"
          @card-click="activeCardId = $event"
          @create-card="openCardEditorFromSelection"
          @save-content="(notePath, content) => void saveNoteContent(notePath, content)"
        />
        <div v-else class="flex h-full items-center justify-center text-[13px] text-ink-3">
          选择或创建一篇笔记
        </div>
      </main>

      <aside
        class="shrink-0 overflow-hidden border-l border-hairline bg-bg-side transition-[width] duration-200 max-md:fixed max-md:inset-y-0 max-md:right-0 max-md:z-40 max-md:shadow-xl"
        :class="panelOpen ? 'w-[300px]' : 'w-0 border-l-0'"
      >
        <div class="h-full w-[300px]">
          <CardPanel
            v-if="activeNote"
            :note-path="activeNote.path"
            :cards="activeNoteCards"
            :blocks="blocks"
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
      <!-- 窄窗口右侧抽屉遮罩 -->
      <div v-if="panelOpen" class="fixed inset-0 z-30 bg-ink/30 md:hidden" @click="panelOpen = false" />
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
    <CommandPalette
      v-if="paletteOpen"
      :notes="notes"
      @close="paletteOpen = false"
      @select-note="(path) => { void selectNote(path); paletteOpen = false; }"
      @select-card="(notePath, cardId) => { void selectNote(notePath); activeCardId = cardId; paletteOpen = false; }"
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
      :providers="providers"
      :active-provider-id="activeProviderId"
      :vault-status="vaultStatus"
      :ai-grading-enabled="aiGradingEnabled"
      :attachment-folder="vaultConfig.attachmentFolder"
      :attachment-folder-saving="attachmentFolderSaving"
      :attachment-folder-status="attachmentFolderStatus"
      @close="settingsOpen = false"
      @update-theme="theme = $event"
      @update-font-size="(size) => void setFontSize(size)"
      @change-vault="leaveVault"
      @set-active-provider="(id) => void setActiveProvider(id)"
      @set-vault-password="(password) => void setVaultPassword(password)"
      @update-ai-grading="(enabled) => void setAiGradingEnabled(enabled)"
      @save-attachment-folder="(folder) => void setAttachmentFolder(folder)"
    />

    <AiSplitDialog
      v-if="aiSplit.open"
      :kind="activeNoteCards[0]?.kind ?? 'qa'"
      :note-title="activeNote?.title ?? ''"
      :note-content="activeNoteContent"
      :has-selection="aiSplit.hasSelection"
      :selection-text="aiSplit.selectionText"
      :provider-configured="providers.some((provider) => provider.hasApiKey || provider.hasCredential)"
      @close="aiSplit.open = false"
      @open-settings="() => { aiSplit.open = false; settingsOpen = true; }"
      @error="showToast"
      @adopt="(drafts) => void handleAiSplitAdopt(drafts)"
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
      @close="cardEditor.open = false"
      @save="(draft) => void handleCardEditorSave(draft)"
    />

    <ReviewOverlay
      v-if="reviewSession.open"
      :title="reviewSession.title"
      :note-path="reviewSession.notePath"
      :include-all="reviewSession.includeAll"
      @close="reviewSession.open = false"
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
