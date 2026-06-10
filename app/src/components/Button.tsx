import type { ButtonHTMLAttributes, ReactNode } from "react";
import { cn } from "../lib/cn.ts";

type Variant = "default" | "primary" | "ghost" | "danger";
type Size = "sm" | "md";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  children: ReactNode;
}

const VARIANTS: Record<Variant, string> = {
  default:
    "bg-surface-2 text-text border border-border hover:border-muted/60 hover:bg-[#22262b]",
  primary:
    "bg-accent text-[#04130c] border border-transparent hover:bg-accent-dim font-medium",
  ghost: "bg-transparent text-muted border border-transparent hover:text-text hover:bg-surface-2",
  danger:
    "bg-transparent text-danger border border-transparent hover:bg-danger/10",
};

const SIZES: Record<Size, string> = {
  sm: "h-7 px-2.5 text-xs gap-1.5",
  md: "h-9 px-3.5 text-sm gap-2",
};

export function Button({
  variant = "default",
  size = "md",
  className,
  children,
  ...rest
}: ButtonProps) {
  return (
    <button
      className={cn(
        "inline-flex items-center justify-center rounded-md select-none",
        "transition-colors duration-150 outline-none",
        "focus-visible:ring-2 focus-visible:ring-accent/50",
        "disabled:opacity-40 disabled:pointer-events-none",
        VARIANTS[variant],
        SIZES[size],
        className,
      )}
      {...rest}
    >
      {children}
    </button>
  );
}
