/**
 * TE Provider 运行态 API（只读）。
 *
 * 只读取注入器的 `/healthz` 与 `/provider-health`：
 * - `online === null` 表示「未配置探针 / 未知」，不是离线；
 * - `runtimeBindingExposed` 恒为 false：task/lease/session/binding 由注入器进程内存持有，
 *   刻意不通过 HTTP 暴露，CCSM 也绝不从别处拼凑这些运行时授权对象。
 */
import { invoke } from "@tauri-apps/api/core";

export interface TeProviderHealthSnapshot {
  /** null 表示未知（例如未配置保活探针），不代表离线。 */
  online: boolean | null;
  reason: string | null;
  checkedAt: string | null;
  httpStatus: number | null;
}

export interface TeProviderRuntimeStatus {
  sidecarUrl: string;
  sidecarReachable: boolean;
  sidecarStatus: string | null;
  /** 稳定错误分类（connect_failed / unexpected_status_<code> / invalid_response）。 */
  sidecarError: string | null;
  provider: TeProviderHealthSnapshot | null;
  latencyMs: number;
  checkedAt: string;
  runtimeBindingExposed: boolean;
}

/** 读取 TE Provider 运行态；端点必须是数值回环 HTTP，校验在 Tauri 侧再执行一次。 */
export async function getTeProviderRuntimeStatus(
  sidecarUrl: string,
): Promise<TeProviderRuntimeStatus> {
  try {
    return await invoke<TeProviderRuntimeStatus>("te_provider_runtime_status", {
      sidecarUrl,
    });
  } catch (error) {
    // Tauri invoke 的拒绝值是字符串；统一成 Error，避免 UI 显示 "[object Object]"。
    throw new Error(typeof error === "string" ? error : String(error));
  }
}
