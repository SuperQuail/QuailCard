/** 图片右键菜单请求：编辑器只上报引用与视口坐标，菜单如何展示由壳层决定。 */
export interface ImageMenuRequest {
  /** 引用所在的笔记路径，后端据此解析相对图片路径。 */
  notePath: string;
  /** Markdown 中的原始图片地址。 */
  source: string;
  /** 视口内的横坐标。 */
  x: number;
  /** 视口内的纵坐标。 */
  y: number;
}

/** 菜单默认尺寸：三项变体的实测宽高（与文件树菜单同一套行高），用于贴边钳制。 */
export const IMAGE_MENU_SIZE = { width: 150, height: 118 } as const;

/**
 * 把右键坐标钳制进视口，保证菜单完整可见。
 *
 * 纯函数：视口尺寸由调用方注入，避免领域层依赖 window。
 */
export function clampImageMenuPosition(
  x: number,
  y: number,
  viewport: { width: number; height: number },
  size: { width: number; height: number } = IMAGE_MENU_SIZE,
): { x: number; y: number } {
  return {
    x: Math.max(0, Math.min(x, viewport.width - size.width)),
    y: Math.max(0, Math.min(y, viewport.height - size.height)),
  };
}
