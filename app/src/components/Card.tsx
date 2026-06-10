import type { ReactNode } from "react";
import { cn } from "../lib/cn.ts";

interface CardProps {
  className?: string;
  children: ReactNode;
}

export function Card({ className, children }: CardProps) {
  return (
    <div
      className={cn(
        "rounded-md border border-border bg-surface",
        className,
      )}
    >
      {children}
    </div>
  );
}

interface CardHeaderProps {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
}

export function CardHeader({ title, subtitle, actions }: CardHeaderProps) {
  return (
    <div className="flex items-center justify-between gap-3 border-b border-border px-4 py-3">
      <div className="min-w-0">
        <h2 className="text-sm font-semibold text-text">{title}</h2>
        {subtitle ? (
          <p className="mt-0.5 truncate text-xs text-muted">{subtitle}</p>
        ) : null}
      </div>
      {actions ? <div className="flex items-center gap-2">{actions}</div> : null}
    </div>
  );
}
