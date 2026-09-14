import { mount } from "@vue/test-utils";
import { describe, expect, test } from "vitest";
import type { VideoProbe, VideoSettings } from "../../domain/video";
import { formatBytes, formatDuration } from "../../domain/video";
import VideoLinkForm from "./VideoLinkForm.vue";
import ComponentPanel from "./ComponentPanel.vue";

/** 每次创建独立解析结果，避免响应式更新污染其他用例。 */
function probe(overrides: Partial<VideoProbe> = {}): VideoProbe {
  return {
    bvid: "BVfixture", title: "测试视频", owner: "作者", duration: 60, loggedIn: false,
    pages: [{ page: 1, title: "第一节", duration: 60 }, { page: 2, title: "第二节", duration: 120 }],
    qualities: [{ qn: 32, label: "480P", height: 480, available: true, requiresVip: false, estimatedBytes: 10 * 1024 * 1024 }],
    ...overrides,
  };
}

/** 只提供展示组件需要的输入，不引入应用状态或后端。 */
function formProps() {
  return {
    url: "BVfixture", probe: probe(), loggedIn: false, forceTranscribe: false, mode: "note" as const,
    quality: 32, pages: [1, 2], screenshots: false, probing: false, starting: false, running: false,
  };
}

/** 模型选择与安装状态独立，默认选中尚未安装的模型。 */
function settings(): VideoSettings {
  return {
    noteFolder: "视频笔记", asrModel: "small", asrEnabled: true, screenshotsEnabled: false,
    maxShots: 10, videoQuality: "auto", preferCodec: "auto", videoMaxDownloadMb: 500,
    shotMaxWidth: 1280, ffmpegPath: "", whisperPath: "", modelMirror: "", keepMediaDays: 7,
  };
}

describe("视频链接控件", () => {
  test("登录状态以当前 prop 为准，提示不承诺登录解锁", async () => {
    const unavailable = probe({ qualities: [{ ...probe().qualities[0]!, available: false }] });
    const wrapper = mount(VideoLinkForm, { props: { ...formProps(), probe: unavailable, loggedIn: true } });
    expect(wrapper.text()).toContain("已登录");
    expect(wrapper.get("option").text()).toContain("当前账号或视频不支持");
    expect(wrapper.text()).not.toContain("请先登录 B 站账号");
    await wrapper.setProps({ loggedIn: false, probe: { ...unavailable, loggedIn: true } });
    expect(wrapper.text()).toContain("未登录");
    expect(wrapper.get("option").text()).toContain("登录后重试（仍可能受账号或视频限制）");
    await wrapper.setProps({ probe: { ...unavailable, qualities: [{ ...unavailable.qualities[0]!, requiresVip: true }] } });
    expect(wrapper.get("option").text()).toContain("需要大会员");
    await wrapper.setProps({ probe: { ...unavailable, qualities: [{ ...unavailable.qualities[0]!, unavailableReason: "该分 P 无此清晰度" }] } });
    expect(wrapper.get("option").text()).toContain("该分 P 无此清晰度");
    wrapper.unmount();
  });

  test("不可用或未知清晰度、空或失效分 P 都不能开始", async () => {
    const wrapper = mount(VideoLinkForm, { props: formProps() });
    const start = wrapper.get("button.primary-btn");
    for (const patch of [
      { probe: probe({ qualities: [{ ...probe().qualities[0]!, available: false }] }) },
      { probe: probe(), quality: 999 },
      { quality: 32, pages: [] },
      { pages: [99] },
    ]) {
      await wrapper.setProps(patch);
      expect(start.attributes("disabled")).toBeDefined();
      await start.trigger("click");
      expect(wrapper.emitted("start")).toBeUndefined();
    }
    await wrapper.setProps({ pages: [1] });
    expect(start.attributes("disabled")).toBeUndefined();
    await start.trigger("click");
    expect(wrapper.emitted("start")).toEqual([[]]);
    wrapper.unmount();
  });

  test("按所选分 P 汇总时长并按估算基准缩放大小", async () => {
    const wrapper = mount(VideoLinkForm, { props: formProps() });
    expect(wrapper.text()).toContain("总时长：" + formatDuration(180));
    expect(wrapper.get("option").text()).toContain("约 " + formatBytes(30 * 1024 * 1024));
    await wrapper.setProps({ pages: [2] });
    expect(wrapper.text()).toContain("总时长：" + formatDuration(120));
    expect(wrapper.get("option").text()).toContain("约 " + formatBytes(20 * 1024 * 1024));
    await wrapper.setProps({ probe: probe({ qualities: [{ ...probe().qualities[0]!, estimatedDuration: 120 }] }) });
    expect(wrapper.get("option").text()).toContain("约 " + formatBytes(10 * 1024 * 1024));
    await wrapper.setProps({ probe: probe({ duration: 0 }) });
    expect(wrapper.get("option").text()).toContain("估算不可用");
    await wrapper.setProps({ probe: probe({ qualities: [{ ...probe().qualities[0]!, estimatedBytes: 0 }] }) });
    expect(wrapper.get("option").text()).toContain("估算不可用");
    wrapper.unmount();
  });

  test("强制转写复选框发出独立更新事件", async () => {
    const wrapper = mount(VideoLinkForm, { props: formProps() });
    const checkbox = wrapper.findAll("label").find(label => label.text().includes("强制本地转写"))!.get('input[type="checkbox"]');
    await checkbox.setValue(true);
    expect(wrapper.emitted("update:forceTranscribe")).toEqual([[true]]);
    expect(wrapper.emitted("update:screenshots")).toBeUndefined();
    await wrapper.setProps({ forceTranscribe: true });
    expect((checkbox.element as HTMLInputElement).checked).toBe(true);
    wrapper.unmount();
  });

  test("切到字幕模式后截图复选框禁用且不改写截图取值", async () => {
    const wrapper = mount(VideoLinkForm, { props: formProps() });
    /** 截图复选框按标签文本定位，禁用契约只取决于输出模式。 */
    const screenshot = () => wrapper.findAll("label").find(label => label.text().includes("生成关键画面截图"))!.get('input[type="checkbox"]');
    /** 字幕单选同样按标签文本定位，避免依赖 DOM 顺序。 */
    const transcript = () => wrapper.findAll("label").find(label => label.text().includes("字幕："))!.get('input[type="radio"]');
    expect(screenshot().attributes("disabled")).toBeUndefined();
    await transcript().setValue();
    await wrapper.setProps({ mode: "transcript" });
    expect(screenshot().attributes("disabled")).toBeDefined();
    expect(wrapper.text()).toContain("字幕模式不使用截图");
    // 禁用只影响交互，不得改写 screenshots 的值；切回笔记模式要恢复原样。
    expect(wrapper.emitted("update:screenshots")).toBeUndefined();
    expect((screenshot().element as HTMLInputElement).checked).toBe(false);
    await wrapper.setProps({ mode: "note" });
    expect(screenshot().attributes("disabled")).toBeUndefined();
    wrapper.unmount();
  });

  test("输出模式单选向父级发出 update:mode", async () => {
    const wrapper = mount(VideoLinkForm, { props: formProps() });
    // DOM 顺序固定为笔记在前、字幕在后，用序数取单选可同时校验默认顺序。
    const radios = wrapper.findAll('input[type="radio"]');
    await radios[1]!.setValue();
    expect(wrapper.emitted("update:mode")).toEqual([["transcript"]]);
    await wrapper.setProps({ mode: "transcript" });
    await radios[0]!.setValue();
    expect(wrapper.emitted("update:mode")).toEqual([["transcript"], ["note"]]);
    wrapper.unmount();
  });
});

describe("组件与模型控件", () => {
  test("未下载的模型不显示选中状态，就绪后才能选用", async () => {
    const wrapper = mount(ComponentPanel, { props: {
      settings: settings(), components: [], downloading: "",
      models: [{ id: "small", label: "Small", bytes: 1024, status: "invalid" }],
    } });
    /** 模型列表只有模型行，按钮按行取，避免混入设置区的其他按钮。 */
    const rowButtons = (): string[] => wrapper.findAll("li")[0]!.findAll("button").map(button => button.text());
    expect(wrapper.text()).toContain("损坏，需重新下载");
    expect(wrapper.text()).not.toContain("已选择");
    expect(wrapper.text()).not.toContain("使用中");
    expect(rowButtons()).toEqual(["下载"]);
    await wrapper.setProps({ models: [{ id: "small", label: "Small", bytes: 1024, status: "future-status" }] });
    expect(wrapper.text()).toContain("状态未知");
    expect(wrapper.text()).not.toContain("已就绪");
    expect(rowButtons()).toEqual(["下载"]);
    await wrapper.setProps({ models: [{ id: "small", label: "Small", bytes: 1024, status: "ready" }] });
    expect(wrapper.text()).toContain("已就绪");
    expect(rowButtons()).toEqual(["已选择"]);
    wrapper.unmount();
  });

  test("活动下载显示进度并发出取消事件，组件详情保留", async () => {
    const wrapper = mount(ComponentPanel, { props: {
      settings: settings(), models: [], downloading: "small",
      components: [{ id: "whisper", name: "whisper", available: false, source: "", path: "", detail: "组件无法执行" }],
      downloadStatus: { modelId: "small", state: "downloading", downloadedBytes: 1024, totalBytes: 4096, bytesPerSecond: 512, error: null },
    } });
    expect(wrapper.text()).toContain("组件无法执行");
    const progress = wrapper.get('[role="status"]');
    expect(progress.text()).toContain(formatBytes(1024));
    expect(progress.text()).toContain(formatBytes(4096));
    expect(progress.text()).toContain(formatBytes(512) + "/s");
    await wrapper.findAll("button").find(button => button.text() === "取消下载")!.trigger("click");
    expect(wrapper.emitted("cancel-download")).toEqual([[]]);
    expect(wrapper.emitted("download")).toBeUndefined();
    await wrapper.setProps({ downloading: "" });
    expect(wrapper.text()).not.toContain("取消下载");
    wrapper.unmount();
  });

  test("已定位组件显示路径并用作占位符，正常状态的诊断文案不重复展示", async () => {
    const wrapper = mount(ComponentPanel, { props: {
      settings: settings(), models: [], downloading: "",
      components: [
        { id: "ffmpeg", name: "ffmpeg（媒体解码与截图）", available: true, path: "C:/app/resources/ffmpeg/ffmpeg.exe", source: "已定位" },
        { id: "whisper", name: "whisper-cli（本地转写）", available: true, path: "C:/app/resources/whisper/whisper-cli.exe", source: "Vulkan", detail: "Vulkan 已枚举到设备；实际模型计算后端以转写诊断为准，失败自动回退 CPU" },
      ],
    } });
    const inputs = wrapper.findAll("input.field-input");
    expect(inputs[0]!.attributes("placeholder")).toBe("C:/app/resources/ffmpeg/ffmpeg.exe");
    expect(inputs[0]!.attributes("title")).toBe("C:/app/resources/ffmpeg/ffmpeg.exe");
    expect(inputs[1]!.attributes("placeholder")).toBe("C:/app/resources/whisper/whisper-cli.exe");
    expect(wrapper.text()).toContain("C:/app/resources/ffmpeg/ffmpeg.exe");
    expect(wrapper.text()).toContain("C:/app/resources/whisper/whisper-cli.exe");
    expect(wrapper.text()).not.toContain("Vulkan 已枚举到设备");
    await wrapper.setProps({ components: [
      { id: "whisper", name: "whisper-cli（本地转写）", available: false, path: "C:/app/resources/whisper/whisper-cli.exe", source: "未找到", detail: "组件启动失败或自检超时" },
    ] });
    expect(wrapper.findAll("input.field-input")[1]!.attributes("placeholder")).toBe("D:/tools/whisper-cli.exe");
    expect(wrapper.text()).not.toContain("C:/app/resources/whisper/whisper-cli.exe");
    expect(wrapper.text()).toContain("组件启动失败或自检超时");
    wrapper.unmount();
  });

  test("已下载的不再提供下载按钮，未下载的不提供选用按钮", async () => {
    const wrapper = mount(ComponentPanel, { props: {
      settings: settings(), components: [], downloading: "",
      models: [
        { id: "small", label: "small", bytes: 466, status: "ready" },
        { id: "base", label: "base", bytes: 142, status: "missing" },
      ],
    } });
    const rows = wrapper.findAll("li");
    const ready = rows.find(row => row.text().startsWith("small"))!;
    expect(ready.findAll("button").map(button => button.text())).toEqual(["已选择"]);
    const missing = rows.find(row => row.text().startsWith("base"))!;
    expect(missing.findAll("button").map(button => button.text())).toEqual(["下载"]);
    wrapper.unmount();
  });
});
