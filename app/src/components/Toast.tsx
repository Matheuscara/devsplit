import { useEffect, useState } from "react";
import { Info, AlertTriangle, XCircle, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { ipc, type NoticeLevel } from "../lib/ipc.ts";
import { cn } from "../lib/cn.ts";

interface ToastItem {
  id: number;
  level: NoticeLevel;
  message: string;
}

const LEVEL_STYLE: Record<NoticeLevel, string> = {
  info: "border-border bg-surface text-text",
  warn: "border-warn/30 bg-warn/10 text-warn",
  error: "border-danger/30 bg-danger/10 text-danger",
};

const LEVEL_ICON: Record<NoticeLevel, LucideIcon> = {
  info: Info,
  warn: AlertTriangle,
  error: XCircle,
};

const TOAST_TTL_MS = 5000;

export function Toaster() {
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let seq = 0;
    (async () => {
      unlisten = await ipc.onNotice((n) => {
        const id = ++seq;
        setToasts((prev) => [...prev, { id, ...n }].slice(-5));
        setTimeout(
          () => setToasts((prev) => prev.filter((t) => t.id !== id)),
          TOAST_TTL_MS,
        );
      });
    })();
    return () => unlisten?.();
  }, []);

  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2">
      {toasts.map((t) => {
        const Icon = LEVEL_ICON[t.level];
        return (
          <div
            key={t.id}
            className={cn(
              "pointer-events-auto flex items-start gap-2.5 rounded-md border px-3 py-2.5 text-sm shadow-lg shadow-black/30",
              LEVEL_STYLE[t.level],
            )}
            style={{ animation: "devsplit-card-in 160ms ease-out" }}
          >
            <Icon size={15} className="mt-0.5 shrink-0" />
            <span className="min-w-0 flex-1 leading-snug">{t.message}</span>
            <button
              onClick={() =>
                setToasts((prev) => prev.filter((x) => x.id !== t.id))
              }
              className="shrink-0 text-muted transition-colors hover:text-text"
              aria-label="Dispensar"
            >
              <X size={14} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
