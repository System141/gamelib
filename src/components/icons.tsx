// Brand glyphs (lucide has no brand icons) and the app logo.

import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

const base = (size: number | undefined, props: IconProps) => ({
  width: size ?? 16,
  height: size ?? 16,
  viewBox: "0 0 24 24",
  fill: "currentColor",
  "aria-hidden": true,
  ...props,
});

export function WindowsIcon({ size, ...props }: IconProps) {
  return (
    <svg {...base(size, props)}>
      <path d="M3 5.5 10.5 4.4v7.1H3V5.5Zm0 13 7.5 1.1v-7H3v5.9Zm8.4 1.2L21 21v-8.4h-9.6v7.1Zm0-15.4v7.2H21V3l-9.6 1.3Z" />
    </svg>
  );
}

export function AppleIcon({ size, ...props }: IconProps) {
  return (
    <svg {...base(size, props)}>
      <path d="M16.4 12.7c0-2.5 2.1-3.7 2.2-3.8-1.2-1.7-3-2-3.7-2-1.6-.2-3.1.9-3.9.9-.8 0-2-.9-3.4-.9-1.7 0-3.3 1-4.2 2.6-1.8 3.1-.5 7.7 1.3 10.2.9 1.2 1.9 2.6 3.2 2.6 1.3-.1 1.8-.8 3.3-.8 1.6 0 2 .8 3.4.8 1.4 0 2.3-1.3 3.1-2.5 1-1.4 1.4-2.8 1.4-2.9-.1 0-2.7-1-2.7-4.2ZM13.9 5.2c.7-.9 1.2-2 1-3.2-1 0-2.3.7-3 1.6-.7.8-1.3 2-1.1 3.1 1.1.1 2.3-.6 3.1-1.5Z" />
    </svg>
  );
}

export function LinuxIcon({ size, ...props }: IconProps) {
  return (
    <svg {...base(size, props)}>
      <path d="M12 2c-2.3 0-3.6 1.9-3.6 4.4 0 1.3.2 2.2-.6 3.4-1 1.4-2.7 3.4-2.8 6-.1 1.1.2 2.1.9 2.7-.4.5-.9.9-1.5 1.1-.6.3-.5.9 0 1.2.9.5 2.3.5 3.5.1.9-.3 1.4-.3 2.1-.2.7.2 1.4.4 2.3.4.9 0 1.6-.2 2.3-.4.7-.1 1.2-.1 2.1.2 1.2.4 2.6.4 3.5-.1.5-.3.6-.9 0-1.2-.6-.2-1.1-.6-1.5-1.1.7-.6 1-1.6.9-2.7-.1-2.6-1.8-4.6-2.8-6-.8-1.2-.6-2.1-.6-3.4C15.6 3.9 14.3 2 12 2Zm-1.6 4.1c.4 0 .7.5.7 1.1 0 .6-.3 1-.7 1-.4 0-.7-.4-.7-1 0-.6.3-1.1.7-1.1Zm3.2 0c.4 0 .7.5.7 1.1 0 .6-.3 1-.7 1-.4 0-.7-.4-.7-1 0-.6.3-1.1.7-1.1ZM12 9.1c.9 0 2 .6 2 1 0 .5-1.1 1.2-2 1.2s-2-.7-2-1.2c0-.4 1.1-1 2-1Z" />
    </svg>
  );
}

export function SteamIcon({ size, ...props }: IconProps) {
  return (
    <svg {...base(size, props)}>
      <path d="M11.98 2C6.73 2 2.43 6.05 2.02 11.2l5.36 2.22a2.82 2.82 0 0 1 1.6-.5h.16l2.39-3.46v-.05a3.78 3.78 0 1 1 3.78 3.78h-.09l-3.4 2.43.01.13a2.84 2.84 0 0 1-5.62.55L2.4 14.64A10 10 0 1 0 11.98 2Zm-4.7 15.17-1.23-.51a2.13 2.13 0 1 0 1.16-2.9l1.27.53a1.57 1.57 0 1 1-1.2 2.88Zm9.53-7.76a2.52 2.52 0 1 0-5.04 0 2.52 2.52 0 0 0 5.04 0Zm-4.4 0a1.89 1.89 0 1 1 3.78 0 1.89 1.89 0 0 1-3.78 0Z" />
    </svg>
  );
}

export function Logo({ size = 32 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 1024 1024" aria-hidden>
      <defs>
        <linearGradient id="logo-bg" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#1c2b4b" />
          <stop offset="1" stopColor="#0b0f16" />
        </linearGradient>
        <linearGradient id="logo-card" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#66c0f4" />
          <stop offset="1" stopColor="#a78bfa" />
        </linearGradient>
      </defs>
      <rect x="48" y="48" width="928" height="928" rx="216" fill="url(#logo-bg)" />
      <rect x="262" y="262" width="300" height="450" rx="46" fill="#66c0f4" fillOpacity="0.32" transform="rotate(-13 412 487)" />
      <rect x="462" y="262" width="300" height="450" rx="46" fill="#a78bfa" fillOpacity="0.42" transform="rotate(13 612 487)" />
      <rect x="352" y="292" width="320" height="480" rx="50" fill="url(#logo-card)" />
      <path
        d="M478 440 C478 424 495 414 509 422 L614 485 C627 493 627 512 614 520 L509 583 C495 591 478 581 478 565 Z"
        fill="#0b0f16"
        fillOpacity="0.88"
      />
    </svg>
  );
}
