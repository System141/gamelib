// Small shared controls.

import clsx from "clsx";
import type { ReactNode } from "react";

export function SmallButton({
  children,
  onClick,
  icon,
  tone = "default",
  disabled,
  title,
  ariaLabel,
}: {
  children: ReactNode;
  onClick: () => void;
  icon?: ReactNode;
  tone?: "default" | "primary" | "danger";
  disabled?: boolean;
  title?: string;
  ariaLabel?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      title={title}
      aria-label={ariaLabel}
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

/** A small square button with only an icon; `label` is its tooltip and accessible name. */
export function IconButton({
  label,
  icon,
  onClick,
  tone = "default",
}: {
  label: string;
  icon: ReactNode;
  onClick: () => void;
  tone?: "default" | "danger";
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      className={clsx(
        "grid size-7 place-items-center rounded-md ring-1 transition",
        tone === "danger"
          ? "text-danger ring-danger/30 hover:bg-danger/15"
          : "bg-white/4 text-ink-200 ring-white/8 hover:bg-white/10 hover:text-white",
      )}
    >
      {icon}
    </button>
  );
}
