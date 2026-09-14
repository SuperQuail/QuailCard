import { mount } from "@vue/test-utils";
import { describe, expect, test } from "vitest";
import type { AsrProgress, VideoTaskStatus } from "../../domain/video";
import { asrMeasurements } from "../../domain/videoAsrProgress";
import VideoAsrProgress from "./VideoAsrProgress.vue";
import VideoTaskProgress from "./VideoTaskProgress.vue";

/** 固定后端回调观测，测试不依赖客户端时钟推进。 */
function progress(patch: Partial<AsrProgress> = {}): AsrProgress {
  return { page: 2, attempt: 1, percent: 50, totalAudioSeconds: 120, elapsedSeconds: 30, ...patch };
}

describe("Whisper 实测进度", () => {
  test("展示当前分 P 真实百分比和明确标注的近似音频量、平均速度", () => {
    const wrapper = mount(VideoAsrProgress, { props: { progress: progress() } });
    expect(wrapper.text()).toContain("本地转写 P2");
    expect(wrapper.text()).toContain("Whisper 50%");
    expect(wrapper.text()).toContain("音频约 01:00 / 02:00");
    expect(wrapper.text()).toContain("平均约 2.00×");
    expect(wrapper.text()).toContain("非实时瞬时速度");
    expect(wrapper.get("progress").attributes("value")).toBe("50");
    expect(wrapper.text()).not.toContain("剩余");
    wrapper.unmount();
  });

  test("CPU 重试清空旧速度；未知进度不能显示虚构的零或百分比", async () => {
    const wrapper = mount(VideoAsrProgress, { props: { progress: progress() } });
    await wrapper.setProps({ progress: progress({ attempt: 2, percent: null, elapsedSeconds: 0 }) });
    expect(wrapper.text()).toContain("CPU 重试");
    expect(wrapper.text()).toContain("等待 Whisper 报告进度");
    expect(wrapper.find("progress").exists()).toBe(false);
    expect(wrapper.text()).not.toContain("%");
    expect(wrapper.text()).not.toContain("×");
    wrapper.unmount();
  });

  test("缺少时长或有效耗时不计算速度，坏数据不夹到百分之百", () => {
    expect(asrMeasurements(progress({ totalAudioSeconds: null })).speed).toBeNull();
    expect(asrMeasurements(progress({ elapsedSeconds: 0 })).speed).toBeNull();
    expect(asrMeasurements(progress({ elapsedSeconds: Number.NaN })).speed).toBeNull();
    expect(asrMeasurements(progress({ totalAudioSeconds: Infinity })).processed).toBeNull();
    expect(asrMeasurements(progress({ percent: 101 })).percent).toBeNull();
    expect(asrMeasurements(progress({ percent: 0 })).percent).toBe(0);
    expect(asrMeasurements(progress({ percent: 0 })).speed).toBeNull();
  });

  test("工作区优先显示原始 Whisper 进度，不能把加权流程值当转写完成量", async () => {
    const task: VideoTaskStatus = {
      taskId: "one", state: "running", step: "本地转写", progress: 45, message: "", sequence: 2,
      segments: 0, shots: 0, transcriptSource: "", notePath: null, error: null, asrProgress: progress(),
    };
    const wrapper = mount(VideoTaskProgress, { props: { task } });
    expect(wrapper.text()).toContain("Whisper 50%");
    expect(wrapper.text()).not.toContain("45%");
    await wrapper.setProps({ task: { ...task, state: "failed", error: "转写失败" } });
    expect(wrapper.find('[data-testid="asr-progress"]').exists()).toBe(false);
    expect(wrapper.text()).toContain("转写失败");
    wrapper.unmount();
  });
});
