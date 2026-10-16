// 轻量平台检测，避免在 SSR 或无 navigator 的环境报错
export const isMac = (): boolean => {
  try {
    const ua = navigator.userAgent || "";
    const plat = (navigator.platform || "").toLowerCase();
    return /mac/i.test(ua) || plat.includes("mac");
  } catch {
    return false;
  }
};

export const isWindows = (): boolean => {
  try {
    const ua = navigator.userAgent || "";
    return /windows|win32|win64/i.test(ua);
  } catch {
    return false;
  }
};

export const isLinux = (): boolean => {
  try {
    const ua = navigator.userAgent || "";
    // WebKitGTK/Chromium 在 Linux/Wayland/X11 下 UA 通常包含 Linux 或 X11
    return (
      /linux|x11/i.test(ua) && !/android/i.test(ua) && !isMac() && !isWindows()
    );
  } catch {
    return false;
  }
};

// Linux 上禁用所有 drag region，规避 Wayland 下 gtk_window_begin_move_drag
// 相关的窗口事件异常（Tauri #13440）。Windows 保留 Tauri 的事件拖拽属性，
// 但不使用原生 CSS 拖拽区，避免 WebView2 更新拖拽区时消耗主线程或挂起。
// macOS 保留原生 CSS 拖动行为。
//
// 这些常量通过 JSX 属性 spread 消费（`{...DRAG_REGION_ATTR}`）。禁用时
// 完全不渲染属性；空字符串或 "false" 仍会被识别为拖拽区域。
export const DRAG_REGION_ENABLED = !isLinux();

export const DRAG_REGION_ATTR: Record<string, unknown> = DRAG_REGION_ENABLED
  ? { "data-tauri-drag-region": true }
  : {};

// 原生 CSS 拖拽只用于 macOS；Windows 使用 Tauri 事件处理拖拽。
export const DRAG_REGION_STYLE: Record<string, unknown> = isMac()
  ? { WebkitAppRegion: "drag" }
  : {};
