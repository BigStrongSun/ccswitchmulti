import { createContext, useContext } from "react";
import type { DialogLayer } from "./layers";

interface OverlayLayerContextValue {
  /**
   * 子组件里未显式指定层级的 Dialog 应使用的默认层级。
   * 全屏面板内部的弹窗需要高于面板本身，否则 portal 到 body 后会被面板遮住。
   */
  dialogLayer?: DialogLayer;
  /**
   * 子组件里再次打开 FullScreenPanel 时使用的默认层级。
   * 这让“向导 -> 新增 provider -> 统一供应商表单”这类嵌套面板保持可见。
   */
  childPanelZIndexClassName?: string;
}

const OverlayLayerContext = createContext<OverlayLayerContextValue>({});

/**
 * 读取当前弹层栈上下文。
 * 返回值只描述默认层级，组件仍可通过自己的 zIndex / zIndexClassName 显式覆盖。
 */
export function useOverlayLayerContext(): OverlayLayerContextValue {
  return useContext(OverlayLayerContext);
}

export { OverlayLayerContext };
