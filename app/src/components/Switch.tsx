import { cn } from "../lib/cn.ts";

interface SwitchProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  disabled?: boolean;
  "aria-label"?: string;
}

export function Switch({
  checked,
  onChange,
  disabled,
  "aria-label": ariaLabel,
}: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full",
        "transition-colors duration-150 outline-none",
        "focus-visible:ring-2 focus-visible:ring-accent/50",
        "disabled:opacity-40 disabled:pointer-events-none",
        checked ? "bg-accent" : "bg-[#2c3036]",
      )}
    >
      <span
        className={cn(
          "inline-block h-4 w-4 rounded-full bg-white shadow-sm",
          "transition-transform duration-150",
          checked ? "translate-x-4" : "translate-x-0.5",
        )}
      />
    </button>
  );
}
