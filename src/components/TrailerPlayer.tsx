// Plays a Steam trailer (an HLS stream) over the game details: natively where the web view plays
// HLS (macOS), otherwise through hls.js, which is loaded only when a trailer is first opened.

import { ExternalLink, LoaderCircle, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { tr } from "../i18n/tr";
import type { Trailer } from "../lib/types";

type State = "loading" | "ready" | "failed";

export function TrailerPlayer({ trailer, onClose, onOpenSteam }: { trailer: Trailer; onClose: () => void; onOpenSteam: () => void }) {
  const video = useRef<HTMLVideoElement>(null);
  const [state, setState] = useState<State>("loading");

  useEffect(() => {
    const el = video.current;
    if (!el) return;
    let disposed = false;
    let destroy = () => {};
    setState("loading");
    const play = () => void el.play().catch(() => undefined);
    if (el.canPlayType("application/vnd.apple.mpegurl")) {
      el.src = trailer.stream;
      play();
    } else {
      import("hls.js")
        .then(({ default: Hls }) => {
          if (disposed) return;
          if (!Hls.isSupported()) return setState("failed");
          // No worker: Steam's streams need no transmuxing, and a worker would need a looser CSP.
          const hls = new Hls({ enableWorker: false, capLevelToPlayerSize: true });
          destroy = () => hls.destroy();
          hls.on(Hls.Events.ERROR, (_event, data) => {
            if (data.fatal) setState("failed");
          });
          hls.on(Hls.Events.MANIFEST_PARSED, play);
          hls.loadSource(trailer.stream);
          hls.attachMedia(el);
        })
        .catch(() => {
          if (!disposed) setState("failed");
        });
    }
    return () => {
      disposed = true;
      destroy();
      el.removeAttribute("src");
      el.load();
    };
  }, [trailer.stream]);

  return (
    <div
      className="animate-fade-in fixed inset-0 z-50 flex flex-col items-center justify-center gap-4 bg-ink-950/95 p-10"
      onClick={onClose}
      role="dialog"
      aria-label={trailer.name || tr.media.trailer}
    >
      <div className="relative aspect-video w-full max-w-6xl" onClick={(e) => e.stopPropagation()}>
        <video
          ref={video}
          controls
          playsInline
          poster={trailer.poster ?? undefined}
          onCanPlay={() => setState((s) => (s === "failed" ? s : "ready"))}
          onError={() => setState("failed")}
          className="size-full rounded-lg bg-black shadow-2xl shadow-black"
        />
        {state === "loading" && (
          <div className="pointer-events-none absolute inset-0 grid place-items-center">
            <LoaderCircle size={36} className="animate-spin text-white/80" />
          </div>
        )}
        {state === "failed" && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 rounded-lg bg-black/80 text-ink-100">
            <p>{tr.media.playerError}</p>
            <button
              type="button"
              onClick={onOpenSteam}
              className="inline-flex items-center gap-1.5 rounded-md bg-white/10 px-3 py-1.5 text-sm ring-1 ring-white/15 hover:bg-white/15"
            >
              {tr.media.watchOnSteam}
              <ExternalLink size={13} />
            </button>
          </div>
        )}
      </div>
      {trailer.name && <div className="text-sm text-ink-200">{trailer.name}</div>}
      <button
        type="button"
        onClick={onClose}
        className="glass absolute top-6 right-6 grid size-10 place-items-center rounded-full ring-1 ring-white/15 hover:ring-white/40"
        aria-label={tr.detail.close}
      >
        <X size={18} />
      </button>
    </div>
  );
}
