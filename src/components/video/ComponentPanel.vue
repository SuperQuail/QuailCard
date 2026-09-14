<script setup lang="ts">
import { computed } from "vue";
import { Cpu, Download, PackageCheck, RefreshCw, TriangleAlert } from "@lucide/vue";
import type { VideoComponentStatus, VideoDownloadStatus, VideoModelStatus, VideoSettings } from "../../domain/video";
import { formatBytes } from "../../domain/video";

const props = defineProps<{
  settings: VideoSettings | null;
  components: VideoComponentStatus[];
  models: VideoModelStatus[];
  downloading: string;
  downloadStatus?: VideoDownloadStatus | null;
}>();
const emit = defineEmits<{
  save: [patch: Partial<VideoSettings>];
  download: [id: string];
  "cancel-download": [];
  refresh: [];
}>();

/** 设置尚未加载完成时不渲染表单。 */
const ready = computed(() => Boolean(props.settings));
/** 缺失组件时给出提示条。 */
const missing = computed(() => props.components.filter(item => !item.available));

/** 已定位组件的真实路径；输入框留空时用它替代示例占位符，让界面显示当前实际使用的程序。 */
function resolvedPath(id: string): string {
  const item = props.components.find(component => component.id === id);
  return item?.available && item.path ? item.path : "";
}

/** 就绪与选用是独立状态，未知状态不能冒充未下载或已安装。 */
function modelStatusLabel(model: VideoModelStatus): string {
  const labels: Record<string, string> = { ready: "已就绪", missing: "未下载", invalid: "损坏，需重新下载" };
  return Object.prototype.hasOwnProperty.call(labels, model.status) ? labels[model.status]! : "状态未知";
}

/** 文本类设置只在失焦或回车时保存，避免每次击键写盘。 */
function saveText(key: keyof VideoSettings, event: Event): void {
  const value = (event.target as HTMLInputElement).value.trim();
  emit("save", { [key]: value } as Partial<VideoSettings>);
}

/** 数字设置做下限保护。 */
function saveNumber(key: keyof VideoSettings, event: Event, minimum: number): void {
  const value = Number((event.target as HTMLInputElement).value);
  emit("save", { [key]: Number.isFinite(value) ? Math.max(minimum, Math.trunc(value)) : minimum } as Partial<VideoSettings>);
}
</script>

<template>
  <section v-if="ready && settings" class="space-y-4 rounded-2xl border border-hairline bg-bg-paper p-4">
    <div class="flex items-center justify-between gap-2">
      <p class="flex items-center gap-2 text-[13px] font-medium"><Cpu :size="15" :stroke-width="1.8" />组件与设置</p>
      <button class="ghost-btn border border-hairline" @click="emit('refresh')">
        <RefreshCw :size="14" :stroke-width="1.8" />重新检测
      </button>
    </div>

    <div v-if="missing.length" class="flex items-start gap-2 rounded-xl bg-bg-side px-3 py-2 text-[12px] text-ink-2">
      <TriangleAlert :size="14" class="mt-0.5 shrink-0" />
      <span>缺少组件：{{ missing.map(item => item.name).join("、") }}。可在下方指定路径，或安装到系统 PATH 后重新检测。</span>
    </div>

    <ul class="space-y-1 text-[12px]">
      <li v-for="item in components" :key="item.id" class="flex items-center gap-2">
        <PackageCheck :size="14" :class="item.available ? 'text-accent-strong' : 'text-ink-3'" />
        <div class="min-w-0 flex-1">
          <p class="truncate">{{ item.name }}</p>
          <p v-if="item.available && item.path" class="truncate text-ink-3" :title="item.path">{{ item.path }}</p>
          <p v-if="!item.available && item.detail" class="break-words text-ink-3">{{ item.detail }}</p>
        </div>
        <span class="shrink-0 text-ink-3">{{ item.available ? item.source : "未找到" }}</span>
      </li>
    </ul>

    <div class="grid gap-2 sm:grid-cols-2">
      <label class="space-y-1 text-[12px] text-ink-2">
        ffmpeg 路径（留空自动查找）
        <input
          class="field-input"
          :value="settings.ffmpegPath"
          :placeholder="resolvedPath('ffmpeg') || 'D:/tools/ffmpeg.exe'"
          :title="resolvedPath('ffmpeg')"
          @change="saveText('ffmpegPath', $event)"
        />
      </label>
      <label class="space-y-1 text-[12px] text-ink-2">
        whisper-cli 路径（留空自动查找）
        <input
          class="field-input"
          :value="settings.whisperPath"
          :placeholder="resolvedPath('whisper') || 'D:/tools/whisper-cli.exe'"
          :title="resolvedPath('whisper')"
          @change="saveText('whisperPath', $event)"
        />
      </label>
      <label class="space-y-1 text-[12px] text-ink-2">
        笔记目录
        <input class="field-input" :value="settings.noteFolder" placeholder="视频笔记" @change="saveText('noteFolder', $event)" />
      </label>
      <label class="space-y-1 text-[12px] text-ink-2">
        模型下载镜像（留空用 HuggingFace）
        <input class="field-input" :value="settings.modelMirror" placeholder="https://hf-mirror.com" @change="saveText('modelMirror', $event)" />
      </label>
      <label class="space-y-1 text-[12px] text-ink-2">
        默认清晰度
        <select class="field-input" :value="settings.videoQuality" @change="emit('save', { videoQuality: ($event.target as HTMLSelectElement).value })">
          <option value="auto">自动（480P，不可用则降档）</option>
          <option value="80">1080P</option>
          <option value="64">720P</option>
          <option value="32">480P</option>
          <option value="16">360P</option>
        </select>
      </label>
      <label class="space-y-1 text-[12px] text-ink-2">
        超过该体积改用远端取帧（MB）
        <input class="field-input" type="number" min="50" :value="settings.videoMaxDownloadMb" @change="saveNumber('videoMaxDownloadMb', $event, 50)" />
      </label>
      <label class="space-y-1 text-[12px] text-ink-2">
        截图数量上限
        <input class="field-input" type="number" min="0" :value="settings.maxShots" @change="saveNumber('maxShots', $event, 0)" />
      </label>
      <label class="space-y-1 text-[12px] text-ink-2">
        临时媒体保留天数
        <input class="field-input" type="number" min="0" :value="settings.keepMediaDays" @change="saveNumber('keepMediaDays', $event, 0)" />
      </label>
    </div>

    <div class="space-y-1">
      <div class="flex flex-wrap items-center gap-2 text-[12px]">
        <input
          id="video-asr-enabled"
          type="checkbox"
          :checked="settings.asrEnabled"
          @change="emit('save', { asrEnabled: ($event.target as HTMLInputElement).checked })"
        />
        <label for="video-asr-enabled">无字幕时使用本地转写</label>
        <input
          id="video-screenshots-enabled"
          type="checkbox"
          :checked="settings.screenshotsEnabled"
          @change="emit('save', { screenshotsEnabled: ($event.target as HTMLInputElement).checked })"
        />
        <label for="video-screenshots-enabled">默认生成关键画面截图</label>
      </div>
    </div>

    <div class="space-y-2 border-t border-hairline pt-3">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <p class="text-[12px] text-ink-2">语音模型（首次使用需下载，存放在应用数据目录）</p>
        <button v-if="downloading" class="ghost-btn border border-hairline" @click="emit('cancel-download')">取消下载</button>
      </div>
      <p v-if="downloading && downloadStatus && downloadStatus.modelId === downloading" class="text-[12px] text-ink-2" role="status">
        已下载 {{ formatBytes(downloadStatus.downloadedBytes) }} /
        {{ downloadStatus.totalBytes && downloadStatus.totalBytes > 0 ? formatBytes(downloadStatus.totalBytes) : "总大小未知" }}
        · {{ formatBytes(downloadStatus.bytesPerSecond) }}/s
      </p>
      <ul class="space-y-1">
        <li v-for="model in models" :key="model.id" class="flex items-center gap-2 text-[12px]">
          <span class="min-w-0 flex-1 truncate">{{ model.label }}</span>
          <span class="shrink-0 text-ink-3">{{ modelStatusLabel(model) }} · {{ formatBytes(model.bytes) }}</span>
          <button
            v-if="model.status !== 'ready'"
            class="ghost-btn border border-hairline shrink-0"
            :disabled="Boolean(downloading)"
            @click="emit('download', model.id)"
          >
            <Download :size="13" :stroke-width="1.8" />{{ downloading === model.id ? "下载中…" : "下载" }}
          </button>
          <button
            v-if="model.status === 'ready'"
            class="ghost-btn shrink-0"
            :class="{ 'border border-hairline': settings.asrModel !== model.id }"
            @click="emit('save', { asrModel: model.id })"
          >
            {{ settings.asrModel === model.id ? "已选择" : "选用" }}
          </button>
        </li>
      </ul>
    </div>
  </section>
</template>
