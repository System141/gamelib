// Small shared controls.

import clsx from "clsx";
import type { ReactNode } from "react";

export function SmallButton({
  children,
  onClick,
  icon,
  tone = "default",
  disabled,
}: {
  children: ReactNode;
  onClick: () => void;
  icon?: ReactNode;
  tone?: "default" | "primary" | "danger";
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={clsx(
        "inline-flex h-7.5 items-center gap-1.5 rounded-lg px-2.5 text-[12.5px] font-medium ring-1 transition disabled:opacity-60",
        tone === "primary" && "bg-accent/15 text-accent-soft ring-accent/35 hover:bg-accent/25",
        tone === "danger" && "bg-danger/15 text-danger ring-danger/35 hover:bg-danger/25",
        tone === "default" && "bg-white/4 text-ink-200 ring-white/8 hover:bg-white/8 hover:text-white",
      )}
    >
      {icon}
      {children}
    </button>
  );
}
