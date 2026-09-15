import { useEffect } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import { usageKeys } from "@/lib/query/usage";
import type { SessionCollectionStatus } from "@/types/usage";

/**
 * 监听后端 `usage-log-recorded` 事件，收到后立刻 invalidate 所有
 * UsageDashboard 相关查询，让用户无需等待 30s 轮询周期。
 *
 * 后端在 `proxy_request_logs` 写入新行时会 emit 该事件（200ms 防抖合并），
 * 来源覆盖代理日志、Claude/Codex/Gemini 会话同步、启动归档。
 *
 * 该 hook 挂在 App 一次，所有页面复用同一个事件订阅；未激活的面板只会
 * 保持缓存失效，不会因此启动查询或会话解析。
 */
export function useUsageEventBridge() {
  const queryClient = useQueryClient();

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let collectionUnlisten: UnlistenFn | undefined;
    let disposed = false;
    let latestCollectionRevision = -1;

    const register = (
      subscription: Promise<UnlistenFn>,
      setUnlisten: (callback: UnlistenFn) => void,
    ) => {
      void subscription
        .then((off) => {
          if (disposed) {
            off();
          } else {
            setUnlisten(off);
          }
        })
        .catch((error) => {
          // A single unavailable Tauri event must not prevent the other
          // listener from registering, nor become an unhandled rejection.
          console.debug("[usage-event-bridge] Failed to subscribe", error);
        });
    };

    register(
      listen("usage-log-recorded", () => {
        if (disposed) return;
        // invalidate 整个 usage 命名空间：summary / trends / providerStats /
        // modelStats / logs 全部跟着重拉
        queryClient.invalidateQueries({ queryKey: usageKeys.all });
      }),
      (off) => {
        unlisten = off;
      },
    );
    register(
      listen<SessionCollectionStatus>(
        "session-collection-updated",
        ({ payload }) => {
          if (disposed) return;
          const revision = payload?.revision;
          if (
            !Number.isFinite(revision) ||
            revision <= latestCollectionRevision
          ) {
            return;
          }
          latestCollectionRevision = revision;
          // Event payload is a notification, not a second source of truth.
          // Re-read the lightweight DB snapshot and already-cached stats only.
          queryClient.invalidateQueries({
            queryKey: usageKeys.sessionCollectionStatus(),
          });
          queryClient.invalidateQueries({
            queryKey: [...usageKeys.all, "codex-subagent-stats"],
          });
        },
      ),
      (offCollection) => {
        collectionUnlisten = offCollection;
      },
    );

    return () => {
      disposed = true;
      unlisten?.();
      collectionUnlisten?.();
    };
  }, [queryClient]);
}
