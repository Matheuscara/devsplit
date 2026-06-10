import { cn } from "../lib/cn.ts";

type DotTone = "on" | "off" | "warn";

const TONES: Record<DotTone, string> = {
  on: "bg-accent",
  off: "bg-muted/50",
  warn: "bg-warn",
};

interface StatusDotProps {
  tone: DotTone;
  pulse?: boolean;
  className?: string;
}

export function StatusDot({ tone, pulse, className }: StatusDotProps) {
  return (
    <span className={cn("relative inline-flex h-2 w-2", className)}>
      {pulse && tone === "on" ? (
        <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-accent opacity-60" />
      ) : null}
      <span
        className={cn(
          "relative inline-flex h-2 w-2 rounded-full",
          TONES[tone],
        )}
      />
    </span>
  );
}
