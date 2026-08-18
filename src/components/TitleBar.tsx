import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Window } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import type { TitleBarOrder, TitleBarSide } from "../App";
import { api } from "../lib/api";

interface TitleBarProps {
  controlsSide: TitleBarSide;
  buttonOrder: TitleBarOrder;
  os: string | null;
  translucent: boolean;
}

// De qué lado y en qué orden van min/max/cerrar cuando no es macOS — no
// hay API que lo diga (WM/tema de escritorio, no el SO: KDE puede
// tenerlos a la izquierda y en otro orden según config). Vienen de
// App.tsx (ajustables en Ajustes → Ventana), no se detectan.
//
// `os` viene de App.tsx (único punto de detección, ver ese archivo) en
// vez de pedirse acá también.
export function TitleBar({ controlsSide, buttonOrder, os, translucent }: TitleBarProps) {
  // getCurrentWindow() adentro de useState (perezoso, corre una vez en el
  // primer render) en vez de a nivel de módulo — si el puente de Tauri
  // todavía no está listo cuando el bundle se evalúa, un throw acá no
  // tira abajo el import completo de la app.
  const [appWindow] = useState<Window>(() => getCurrentWindow());
  const [isMaximized, setIsMaximized] = useState(false);
  // Vacío en cualquier SO donde no aplique (ver native_icons.rs) — los
  // botones caen solos al glifo dibujado a mano, no hace falta chequear
  // `os` acá también.
  const [nativeIcons, setNativeIcons] = useState<Record<string, string>>({});

  useEffect(() => {
    appWindow.isMaximized().then(setIsMaximized).catch(() => {});
    const unlisten = appWindow.onResized(() => {
      appWindow.isMaximized().then(setIsMaximized).catch(() => {});
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [appWindow]);

  // Se pide al montar y de nuevo cada vez que native_icons.rs avisa por
  // el portal que el tema de íconos cambió en vivo (Ajustes del sistema),
  // sin necesitar relanzar la app — así la titlebar sigue al tema global.
  useEffect(() => {
    const refresh = () => api.getNativeWindowIcons().then(setNativeIcons).catch(() => {});
    refresh();
    const unlisten = listen("native-window-icons-changed", refresh);
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  if (!os) return <div className="h-9 shrink-0" />;

  const controls = (
    <WindowControls appWindow={appWindow} isMaximized={isMaximized} order={buttonOrder} nativeIcons={nativeIcons} />
  );

  // Sin padding horizontal en el contenedor: los botones nativos (no-mac)
  // tienen que tocar el borde de la ventana, no flotar con margen — así
  // es como se ven de verdad en Windows/KDE. El semáforo de macOS sí lleva
  // su propio margen (pl-3), esa es la convención real ahí.
  return (
    <div
      data-tauri-drag-region
      onDoubleClick={() => appWindow.toggleMaximize()}
      className={`flex h-9 shrink-0 items-center border-b border-zinc-200 text-zinc-500 dark:border-zinc-800 ${
        translucent ? "bg-white/70 dark:bg-zinc-950/60" : "bg-white dark:bg-zinc-950"
      }`}
    >
      {os === "macos" ? (
        <div className="pl-3">
          <TrafficLights appWindow={appWindow} />
        </div>
      ) : (
        controlsSide === "left" && controls
      )}
      <div data-tauri-drag-region className="flex-1 self-stretch" />
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

// Ícono real del tema (mask-image + currentColor, hereda color/hover igual
// que un SVG con stroke=currentColor) si native_icons.rs lo resolvió;
// si no (Windows/macOS, o el tema no lo tiene), el glifo dibujado a mano.
function Glyph({ dataUri, fallback }: { dataUri: string | undefined; fallback: ReactNode }) {
  if (!dataUri) return <>{fallback}</>;
  return (
    <span
      className="block h-3.5 w-3.5"
      style={{
        backgroundColor: "currentColor",
        WebkitMaskImage: `url(${dataUri})`,
        maskImage: `url(${dataUri})`,
        WebkitMaskSize: "contain",
        maskSize: "contain",
        WebkitMaskRepeat: "no-repeat",
        maskRepeat: "no-repeat",
        WebkitMaskPosition: "center",
        maskPosition: "center",
      }}
    />
  );
}

// Trazo 1.5 (no 2) y sin radio/gap en los botones — el estilo grueso y
// "flotante" de antes no imitaba nada real (ni Fluent ni Breeze). Estos
// paths son el fallback para SO donde no hay ícono real que leer.
function WindowControls({
  appWindow,
  isMaximized,
  order,
  nativeIcons,
}: {
  appWindow: Window;
  isMaximized: boolean;
  order: TitleBarOrder;
  nativeIcons: Record<string, string>;
}) {
  const minimize = (
    <button
      key="minimize"
      onClick={() => appWindow.minimize()}
      aria-label="Minimizar"
      className="flex h-9 w-11 items-center justify-center hover:bg-zinc-200/60 dark:hover:bg-zinc-800"
    >
      <Glyph
        dataUri={nativeIcons.minimize}
        fallback={
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="h-3.5 w-3.5">
            <path d="M5 12h14" />
          </svg>
        }
      />
    </button>
  );
  const maximize = (
    <button
      key="maximize"
      onClick={() => appWindow.toggleMaximize()}
      aria-label={isMaximized ? "Restaurar" : "Maximizar"}
      className="flex h-9 w-11 items-center justify-center hover:bg-zinc-200/60 dark:hover:bg-zinc-800"
    >
      <Glyph
        dataUri={isMaximized ? nativeIcons.restore : nativeIcons.maximize}
        fallback={
          isMaximized ? (
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="h-3.5 w-3.5">
              <path d="M8 4h12v12M4 8h12v12H4z" />
            </svg>
          ) : (
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="h-3.5 w-3.5">
              <rect x="4" y="4" width="16" height="16" rx="1" />
            </svg>
          )
        }
      />
    </button>
  );
  const close = (
    <button
      key="close"
      onClick={() => appWindow.close()}
      aria-label="Cerrar"
      className="flex h-9 w-11 items-center justify-center hover:bg-red-600 hover:text-white"
    >
      <Glyph
        dataUri={nativeIcons.close}
        fallback={
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="h-3.5 w-3.5">
            <path d="M18 6 6 18M6 6l12 12" />
          </svg>
        }
      />
    </button>
  );

  return <div className="flex h-full items-center">{order === "closeFirst" ? [close, minimize, maximize] : [minimize, maximize, close]}</div>;
}
