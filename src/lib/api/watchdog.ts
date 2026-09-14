import { invoke } from "@tauri-apps/api/core";

/** 看门狗日志里的一条事件。 */
export interface WatchdogEventRecord {
  timestamp: string;
  level: string;
  event: string;
  detail?: unknown;
}

/** 设置页“看门狗与端口自检”面板的数据。 */
export interface WatchdogStatus {
  enabled: boolean;
  appPid: number;
  supervisorPid?: number | null;
  supervisorAlive: boolean;
  supervisorPath?: string | null;
  restartsInWindow: number;
  port: number;
  listenerPid?: number | null;
  listenerPath?: string | null;
  portReady: boolean;
  portDiagnosis: string;
  recentEvents: WatchdogEventRecord[];
}

export const watchdogApi = {
  async getStatus(): Promise<WatchdogStatus> {
    return await invoke("get_watchdog_status");
  },
};
