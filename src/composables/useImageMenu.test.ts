import { beforeEach, describe, expect, test, vi } from "vitest";
import { useImageMenu } from "./useImageMenu";
import { copyAttachmentImage, revealAttachment, saveAttachmentAs } from "../services/attachmentActions";
import type { ImageMenuRequest } from "../domain/imageMenu";

vi.mock("../services/attachmentActions", () => ({
  copyAttachmentImage: vi.fn(),
  saveAttachmentAs: vi.fn(),
  revealAttachment: vi.fn(),
}));

/** 菜单请求夹具：notePath / source 是动作必须原样转给 services 的两个参数。 */
const request: ImageMenuRequest = {
  notePath: "英语/单词.md",
  source: "../attachments/pic.png",
  x: 120,
  y: 64,
};

/** 构造用例实例并收集提示，断言只关心「服务调用 + 提示内容 + 状态复位」。 */
function menu() {
  const toast = vi.fn();
  return { toast, ...useImageMenu({ showToast: toast }) };
}

beforeEach(() => {
  vi.resetAllMocks();
});

describe("菜单状态", () => {
  test("open 保存请求，close 清空", () => {
    const { context, open, close } = menu();
    expect(context.value).toBeNull();
    open(request);
    expect(context.value).toEqual(request);
    close();
    expect(context.value).toBeNull();
  });

  test("菜单未打开时三个动作都不发请求、不提示", async () => {
    const { toast, copyImage, saveImage, revealImage } = menu();
    await copyImage();
    await saveImage();
    await revealImage();
    expect(copyAttachmentImage).not.toHaveBeenCalled();
    expect(saveAttachmentAs).not.toHaveBeenCalled();
    expect(revealAttachment).not.toHaveBeenCalled();
    expect(toast).not.toHaveBeenCalled();
  });
});

describe("复制图片", () => {
  test("成功提示已复制图片，动作开始时先关闭菜单并置忙碌", async () => {
    const { toast, context, busy, open, copyImage } = menu();
    let closedAtCall: boolean | null = null;
    let busyAtCall: boolean | null = null;
    vi.mocked(copyAttachmentImage).mockImplementation(async () => {
      closedAtCall = context.value === null;
      busyAtCall = busy.value;
    });
    open(request);
    await copyImage();
    expect(copyAttachmentImage).toHaveBeenCalledWith("英语/单词.md", "../attachments/pic.png");
    expect(closedAtCall).toBe(true);
    expect(busyAtCall).toBe(true);
    expect(toast).toHaveBeenCalledWith("已复制图片");
    expect(busy.value).toBe(false);
  });

  test("失败按 code 兜底文案提示，并复位忙碌", async () => {
    const { toast, busy, open, copyImage } = menu();
    vi.mocked(copyAttachmentImage).mockRejectedValue({ code: "CLIPBOARD_ERROR" });
    open(request);
    await copyImage();
    expect(toast).toHaveBeenCalledWith("无法写入剪贴板");
    expect(busy.value).toBe(false);
  });

  test("后端带安全 message 时优先用 message", async () => {
    const { toast, open, copyImage } = menu();
    vi.mocked(copyAttachmentImage).mockRejectedValue({
      code: "CLIPBOARD_ERROR",
      message: "剪贴板正被其他程序占用",
    });
    open(request);
    await copyImage();
    expect(toast).toHaveBeenCalledWith("剪贴板正被其他程序占用");
  });
});

describe("图片另存为", () => {
  test("用户取消（返回 null）时静默，不提示也不留下菜单", async () => {
    const { toast, context, busy, open, saveImage } = menu();
    vi.mocked(saveAttachmentAs).mockResolvedValue(null);
    open(request);
    await saveImage();
    expect(saveAttachmentAs).toHaveBeenCalledWith("英语/单词.md", "../attachments/pic.png");
    expect(toast).not.toHaveBeenCalled();
    expect(context.value).toBeNull();
    expect(busy.value).toBe(false);
  });

  test("保存成功提示包含实际路径", async () => {
    const { toast, open, saveImage } = menu();
    vi.mocked(saveAttachmentAs).mockResolvedValue("C:/Users/demo/Downloads/pic.png");
    open(request);
    await saveImage();
    expect(toast).toHaveBeenCalledTimes(1);
    expect(toast.mock.calls[0]![0]).toContain("C:/Users/demo/Downloads/pic.png");
    expect(toast.mock.calls[0]![0]).toContain("已保存到");
  });

  test("保存失败提示 code 兜底文案", async () => {
    const { toast, busy, open, saveImage } = menu();
    vi.mocked(saveAttachmentAs).mockRejectedValue({ code: "SAVE_ERROR" });
    open(request);
    await saveImage();
    expect(toast).toHaveBeenCalledWith("保存图片失败");
    expect(busy.value).toBe(false);
  });
});

describe("在文件夹中显示", () => {
  test("成功不提示，系统窗口本身就是反馈", async () => {
    const { toast, busy, open, revealImage } = menu();
    vi.mocked(revealAttachment).mockResolvedValue(undefined);
    open(request);
    await revealImage();
    expect(revealAttachment).toHaveBeenCalledWith("英语/单词.md", "../attachments/pic.png");
    expect(toast).not.toHaveBeenCalled();
    expect(busy.value).toBe(false);
  });

  test("失败提示 code 兜底文案，避免点了没反应", async () => {
    const { toast, open, revealImage } = menu();
    vi.mocked(revealAttachment).mockRejectedValue({ code: "IMAGE_DECODE_ERROR" });
    open(request);
    await revealImage();
    expect(toast).toHaveBeenCalledWith("图片无法解码");
  });
});
