import type { ReactNode } from "react";
import { cn } from "../lib/cn.ts";

type Tone = "neutral" | "local" | "passthrough" | "warn" | "danger" | "ok";

const TONES: Record<Tone, string> = {
  neutral: "bg-surface-2 text-muted border-border",
  local: "bg-accent/12 text-accent border-accent/25",
  passthrough: "bg-[#3b82f6]/12 text-[#7cb0ff] border-[#3b82f6]/25",
  warn: "bg-warn/12 text-warn border-warn/25",
  danger: "bg-danger/12 text-danger border-danger/25",
  ok: "bg-accent/12 text-accent border-accent/25",
};

interface BadgeProps {
  tone?: Tone;
  className?: string;
  children: ReactNode;
}

export function Badge({ tone = "neutral", className, children }: BadgeProps) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded-md border px-1.5 py-0.5",
        "text-[11px] font-medium leading-none tracking-wide",
        TONES[tone],
        className,
      )}
    >
      {children}
    </span>
  );
}
