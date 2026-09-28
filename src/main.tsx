import "@fontsource-variable/inter";
import "@fontsource-variable/outfit";
import "./index.css";

import { QueryClientProvider } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ErrorBoundary } from "./components/Feedback";
import { queryClient } from "./lib/queryClient";

async function bootstrap() {
  // Outside Tauri (plain `pnpm dev` in a browser) answer IPC calls from fixture data.
  // `import.meta.env.DEV` is false in production builds, so the mocks are never bundled.
  if (import.meta.env.DEV && !isTauri()) {
    const { installMocks } = await import("./mocks/install");
    await installMocks(new URLSearchParams(window.location.search));
  }

  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <ErrorBoundary>
        <QueryClientProvider client={queryClient}>
          <App />
        </QueryClientProvider>
      </ErrorBoundary>
    </StrictMode>,
  );
}

void bootstrap();
