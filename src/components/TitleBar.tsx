import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Window } from "@tauri-apps/api/window";
import type { TitleBarOrder, TitleBarSide } from "../App";
import { api } from "../lib/api";

interface TitleBarProps {
  controlsSide: TitleBarSide;
  buttonOrder: TitleBarOrder;
}

// De qué lado y en qué orden van min/max/cerrar cuando no es macOS — no
// hay API que lo diga (WM/tema de escritorio, no el SO: KDE puede
// tenerlos a la izquierda y en otro orden según config). Vienen de
// App.tsx (ajustables en Ajustes → Ventana), no se detectan.
export function TitleBar({ controlsSide, buttonOrder }: TitleBarProps) {
  // getCurrentWindow() adentro de useState (perezoso, corre una vez en el
  // primer render) en vez de a nivel de módulo — si el puente de Tauri
  // todavía no está listo cuando el bundle se evalúa, un throw acá no
  // tira abajo el import completo de la app.
  const [appWindow] = useState<Window>(() => getCurrentWindow());
  const [os, setOs] = useState<string | null>(null);
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    api.getOs().then(setOs).catch(() => setOs("linux"));
    appWindow.isMaximized().then(setIsMaximized).catch(() => {});
    const unlisten = appWindow.onResized(() => {
      appWindow.isMaximized().then(setIsMaximized).catch(() => {});
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [appWindow]);

  if (!os) return <div className="h-9 shrink-0" />;

  const controls = <WindowControls appWindow={appWindow} isMaximized={isMaximized} order={buttonOrder} />;

  return (
    <div
      data-tauri-drag-region
      onDoubleClick={() => appWindow.toggleMaximize()}
      className="flex h-9 shrink-0 items-center border-b border-zinc-200 bg-white px-3 text-zinc-500 dark:border-zinc-800 dark:bg-zinc-950"
    >
      {os === "macos" ? <TrafficLights appWindow={appWindow} /> : controlsSide === "left" && controls}
      <div data-tauri-drag-region className="flex-1" />
      {os !== "macos" && controlsSide === "right" && controls}
    </div>
  );
}

function TrafficLights({ appWindow }: { appWindow: Window }) {
  return (
    <div className="flex items-center gap-2">
      <button
        onClick={() => appWindow.close()}
        aria-label="Cerrar"
        className="h-3 w-3 rounded-full bg-[#ff5f57] hover:brightness-90"
      />
      <button
        onClick={() => appWindow.minimize()}
        aria-label="Minimizar"
        className="h-3 w-3 rounded-full bg-[#febc2e] hover:brightness-90"
      />
      <button
        onClick={() => appWindow.toggleMaximize()}
        aria-label="Maximizar"
        className="h-3 w-3 rounded-full bg-[#28c840] hover:brightness-90"
      />
    </div>
  );
}

function WindowControls({
  appWindow,
  isMaximized,
  order,
}: {
  appWindow: Window;
  isMaximized: boolean;
  order: TitleBarOrder;
}) {
  const minimize = (
    <button
      key="minimize"
      onClick={() => appWindow.minimize()}
      aria-label="Minimizar"
      className="rounded-md p-1.5 hover:bg-zinc-200/60 dark:hover:bg-zinc-800"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-3.5 w-3.5">
        <path d="M5 12h14" />
      </svg>
    </button>
  );
  const maximize = (
    <button
      key="maximize"
      onClick={() => appWindow.toggleMaximize()}
      aria-label={isMaximized ? "Restaurar" : "Maximizar"}
      className="rounded-md p-1.5 hover:bg-zinc-200/60 dark:hover:bg-zinc-800"
    >
      {isMaximized ? (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-3.5 w-3.5">
          <path d="M8 4h12v12M4 8h12v12H4z" />
        </svg>
      ) : (
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-3.5 w-3.5">
          <rect x="4" y="4" width="16" height="16" rx="1" />
        </svg>
      )}
    </button>
  );
  const close = (
    <button
      key="close"
      onClick={() => appWindow.close()}
      aria-label="Cerrar"
      className="rounded-md p-1.5 hover:bg-red-600 hover:text-white"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-3.5 w-3.5">
        <path d="M18 6 6 18M6 6l12 12" />
      </svg>
    </button>
  );

  return (
    <div className="flex items-center gap-1">
      {order === "closeFirst" ? [close, minimize, maximize] : [minimize, maximize, close]}
    </div>
  );
}
