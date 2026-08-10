import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api } from "../lib/api";
import type { AiProviderConfig, Indexer, IndexerResult, SourceSettings, TorrentEngineConfig } from "../types";
import type { TitleBarOrder, TitleBarSide } from "../App";
import { AddAiProviderModal } from "./AddAiProviderModal";
import { AddIndexerModal } from "./AddIndexerModal";
import { BITTORRENT_SHARING_NOTICE } from "../lib/legalText";

type SectionId =
  | "ia"
  | "indexers"
  | "almacenamiento"
  | "ventana"
  | "motor"
  | "comunidades"
  | "avanzado"
  | "legal";

interface Section {
  id: SectionId;
  label: string;
  status: "ready" | "soon";
  note?: string;
}

const SECTIONS: Section[] = [
  { id: "ia", label: "IA", status: "ready" },
  { id: "indexers", label: "Indexers", status: "ready" },
  { id: "almacenamiento", label: "Almacenamiento", status: "ready" },
  { id: "ventana", label: "Ventana", status: "ready" },
  { id: "motor", label: "Motor de torrents", status: "ready" },
  {
    id: "comunidades",
    label: "Comunidades",
    status: "soon",
    note: "La capa social vía Nostr (identidad, relés, comunidades NIP-72, moderación local) todavía no está implementada.",
  },
  {
    id: "avanzado",
    label: "Avanzado",
    status: "soon",
    note: "Relé propio auto-hospedable, web-of-trust y ofuscación de tráfico P2P son candidatos de fase futura, sin diseño cerrado todavía.",
  },
  { id: "legal", label: "Aviso legal", status: "ready" },
];

interface AjustesProps {
  onClose: () => void;
  titleBarSide: TitleBarSide;
  onSetTitleBarSide: (side: TitleBarSide) => void;
  titleBarOrder: TitleBarOrder;
  onSetTitleBarOrder: (order: TitleBarOrder) => void;
}

// Panel flotante esmerilado con secciones en acordeón — reemplaza las tabs
// planas de la primera versión a pedido explícito del usuario, coincide con
// los requerimientos reales de frontend ya documentados en el plan
// ("paneles flotantes con glass/backdrop-blur", "secciones colapsables tipo
// acordeón") que el prototipo descartado nunca construyó de verdad.
export function Ajustes({
  onClose,
  titleBarSide,
  onSetTitleBarSide,
  titleBarOrder,
  onSetTitleBarOrder,
}: AjustesProps) {
  const [expanded, setExpanded] = useState<SectionId>("ia");

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex max-h-[85vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-zinc-200/50 bg-white/80 shadow-2xl backdrop-blur-xl dark:border-zinc-800/50 dark:bg-zinc-900/80"
      >
        <div className="flex items-center justify-between border-b border-zinc-200/50 px-4 py-3 dark:border-zinc-800/50">
          <h2 className="text-sm font-semibold text-zinc-900 dark:text-white">Ajustes</h2>
          <button onClick={onClose} className="rounded-md p-1 text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="flex-1 overflow-y-auto">
          {SECTIONS.map((s) => (
            <AccordionSection
              key={s.id}
              section={s}
              isOpen={expanded === s.id}
              onToggle={() => setExpanded((cur) => (cur === s.id ? cur : s.id))}
            >
              {s.status === "soon" ? (
                <p className="p-4 text-xs text-zinc-500 dark:text-zinc-400">{s.note}</p>
              ) : s.id === "ia" ? (
                <IaTab />
              ) : s.id === "indexers" ? (
                <IndexersTab />
              ) : s.id === "almacenamiento" ? (
                <AlmacenamientoTab />
              ) : s.id === "motor" ? (
                <MotorTab />
              ) : s.id === "legal" ? (
                <AvisoLegalTab />
              ) : (
                <VentanaTab
                  titleBarSide={titleBarSide}
                  onSetTitleBarSide={onSetTitleBarSide}
                  titleBarOrder={titleBarOrder}
                  onSetTitleBarOrder={onSetTitleBarOrder}
                />
              )}
            </AccordionSection>
          ))}
        </div>
      </div>
    </div>
  );
}

interface AccordionSectionProps {
  section: Section;
  isOpen: boolean;
  onToggle: () => void;
  children: ReactNode;
}

function AccordionSection({ section, isOpen, onToggle, children }: AccordionSectionProps) {
  return (
    <div className="border-b border-zinc-200/50 last:border-b-0 dark:border-zinc-800/50">
      <button
        onClick={onToggle}
        className="flex w-full items-center justify-between gap-2 px-4 py-3 text-left"
      >
        <span className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-zinc-100">
          {section.label}
          {section.status === "soon" && (
            <span className="rounded-full bg-zinc-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-zinc-500 dark:text-zinc-400">
              Próximamente
            </span>
          )}
        </span>
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          className={`h-4 w-4 text-zinc-400 transition-transform ${isOpen ? "rotate-180" : ""}`}
        >
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>
      {isOpen && <div>{children}</div>}
    </div>
  );
}

function IaTab() {
  const [providers, setProviders] = useState<AiProviderConfig[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);

  function refresh() {
    setLoading(true);
    api
      .listAiProviders()
      .then(setProviders)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }

  useEffect(refresh, []);

  async function activate(id: string) {
    setBusyId(id);
    try {
      await api.setActiveAiProvider(id);
      refresh();
    } catch (e) {
      console.error(`[popcorn] no se pudo activar el proveedor ${id}: ${e}`);
    } finally {
      setBusyId(null);
    }
  }

  async function remove(id: string) {
    setBusyId(id);
    try {
      await api.removeAiProvider(id);
      refresh();
    } catch (e) {
      console.error(`[popcorn] no se pudo quitar el proveedor ${id}: ${e}`);
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div className="flex flex-col gap-3 p-4 pt-0">
      <div className="rounded-lg border border-zinc-200 p-2.5 text-[11px] text-zinc-500 dark:border-zinc-800 dark:text-zinc-400">
        <p className="mb-1 font-semibold text-zinc-600 dark:text-zinc-300">Qué hace / qué no</p>
        <p>
          Hace: interpreta búsquedas en lenguaje natural (título/año/género) y cura resultados por fuente cuando
          está activado. No hace: no navega la web en vivo más allá de lo que el proveedor mismo resuelva en su
          propia llamada, no es detección forense de contenido, nunca sugiere sitios o indexers por su cuenta.
        </p>
      </div>

      <p className="text-xs text-zinc-500 dark:text-zinc-400">
        Se puede guardar más de un proveedor, pero solo uno está activo a la vez — el que usan la búsqueda en
        lenguaje natural y la curación. Las API keys viven en el keychain del sistema, nunca en la base de datos.
      </p>

      <button
        onClick={() => setModalOpen(true)}
        className="self-start rounded-lg bg-[var(--accent)] px-3 py-1.5 text-xs font-semibold text-white hover:bg-[var(--accent-hover)]"
      >
        + Agregar proveedor
      </button>

      {loading && <p className="text-xs text-zinc-500 dark:text-zinc-400">Cargando…</p>}
      {error && <p className="text-xs text-red-500">{error}</p>}
      {!loading && providers.length === 0 && (
        <p className="text-xs text-zinc-500 dark:text-zinc-400">
          No hay proveedores configurados — la búsqueda en lenguaje natural y la curación quedan deshabilitadas
          hasta que agregues uno.
        </p>
      )}

      <ul className="flex flex-col gap-1.5">
        {providers.map((p) => (
          <li
            key={p.id}
            className="flex items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2 dark:border-zinc-800"
          >
            <div className="min-w-0">
              <p className="flex items-center gap-1.5 truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">
                {p.label}
                {p.active && (
                  <span className="rounded-full bg-[var(--accent-soft)] px-1.5 py-0.5 text-[10px] font-semibold text-[var(--accent)] dark:text-[var(--accent-fg)]">
                    Activo
                  </span>
                )}
                {!p.has_api_key && (
                  <span className="rounded-full bg-amber-500/15 px-1.5 py-0.5 text-[10px] font-semibold text-amber-600 dark:text-amber-400">
                    sin key
                  </span>
                )}
              </p>
              <p className="truncate text-[10px] text-zinc-500">
                {p.kind} · {p.model}
                {p.base_url ? ` · ${p.base_url}` : ""}
              </p>
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              {!p.active && (
                <button
                  onClick={() => activate(p.id)}
                  disabled={busyId === p.id}
                  className="rounded-md border border-[var(--accent)] px-2 py-1 text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
                >
                  Activar
                </button>
              )}
              <button
                onClick={() => remove(p.id)}
                disabled={busyId === p.id}
                className="rounded-md border border-red-300 px-2 py-1 text-[11px] text-red-600 hover:bg-red-50 disabled:opacity-50 dark:border-red-900 dark:text-red-400 dark:hover:bg-red-950/40"
              >
                Quitar
              </button>
            </div>
          </li>
        ))}
      </ul>

      {modalOpen && <AddAiProviderModal onClose={() => setModalOpen(false)} onAdded={refresh} />}
    </div>
  );
}

function IndexersTab() {
  const [indexers, setIndexers] = useState<Indexer[]>([]);
  const [sourceSettings, setSourceSettings] = useState<SourceSettings[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [testQuery, setTestQuery] = useState<Record<string, string>>({});
  const [testResults, setTestResults] = useState<Record<string, IndexerResult[] | string>>({});
  const [testingId, setTestingId] = useState<string | null>(null);

  function refresh() {
    setLoading(true);
    Promise.all([api.listIndexers(), api.listSourceSettings()])
      .then(([i, s]) => {
        setIndexers(i);
        setSourceSettings(s);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }

  useEffect(refresh, []);

  async function toggle(id: string, enabled: boolean) {
    setBusyId(id);
    try {
      await api.toggleIndexer(id, enabled);
      refresh();
    } catch (e) {
      console.error(`[popcorn] no se pudo togglear el indexer ${id}: ${e}`);
    } finally {
      setBusyId(null);
    }
  }

  async function toggleCuration(id: string, enabled: boolean) {
    setBusyId(id);
    try {
      await api.setSourceCurationEnabled(id, enabled);
      refresh();
    } catch (e) {
      console.error(`[popcorn] no se pudo togglear curación de ${id}: ${e}`);
    } finally {
      setBusyId(null);
    }
  }

  async function remove(id: string) {
    setBusyId(id);
    try {
      await api.removeIndexer(id);
      refresh();
    } catch (e) {
      console.error(`[popcorn] no se pudo quitar el indexer ${id}: ${e}`);
    } finally {
      setBusyId(null);
    }
  }

  async function test(indexer: Indexer) {
    const query = (testQuery[indexer.id] || "").trim();
    if (!query) return;
    setTestingId(indexer.id);
    try {
      const results = await api.testIndexer(indexer, query);
      setTestResults((r) => ({ ...r, [indexer.id]: results }));
    } catch (e) {
      setTestResults((r) => ({ ...r, [indexer.id]: String(e) }));
    } finally {
      setTestingId(null);
    }
  }

  return (
    <div className="flex flex-col gap-3 p-4 pt-0">
      <p className="text-xs text-zinc-500 dark:text-zinc-400">
        El indexer es tuyo — Popcorn no trae ninguno precargado ni recomienda alguno en particular.
      </p>

      <button
        onClick={() => setModalOpen(true)}
        className="self-start rounded-lg bg-[var(--accent)] px-3 py-1.5 text-xs font-semibold text-white hover:bg-[var(--accent-hover)]"
      >
        + Agregar indexer
      </button>

      {loading && <p className="text-xs text-zinc-500 dark:text-zinc-400">Cargando…</p>}
      {error && <p className="text-xs text-red-500">{error}</p>}
      {!loading && indexers.length === 0 && (
        <p className="text-xs text-zinc-500 dark:text-zinc-400">No hay indexers agregados.</p>
      )}

      <ul className="flex flex-col gap-2">
        {indexers.map((idx) => {
          const settings = sourceSettings.find((s) => s.id === idx.id);
          const result = testResults[idx.id];
          return (
            <li
              key={idx.id}
              className="flex flex-col gap-1.5 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800"
            >
              <div className="flex items-center justify-between gap-2">
                <div className="min-w-0">
                  <p className="truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">{idx.name}</p>
                  <p className="truncate text-[10px] text-zinc-500">{idx.search_url_template}</p>
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  <button
                    onClick={() => toggle(idx.id, !idx.enabled)}
                    disabled={busyId === idx.id}
                    className="rounded-md border border-zinc-300 px-2 py-1 text-[11px] text-zinc-600 hover:bg-zinc-100 disabled:opacity-50 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
                  >
                    {idx.enabled ? "Desactivar" : "Activar"}
                  </button>
                  <button
                    onClick={() => remove(idx.id)}
                    disabled={busyId === idx.id}
                    className="rounded-md border border-red-300 px-2 py-1 text-[11px] text-red-600 hover:bg-red-50 disabled:opacity-50 dark:border-red-900 dark:text-red-400 dark:hover:bg-red-950/40"
                  >
                    Quitar
                  </button>
                </div>
              </div>

              {settings && (
                <label className="flex items-center gap-1.5 text-[11px] text-zinc-600 dark:text-zinc-300">
                  <input
                    type="checkbox"
                    checked={settings.curation_enabled}
                    onChange={(e) => toggleCuration(idx.id, e.target.checked)}
                    disabled={busyId === idx.id}
                  />
                  Curación por IA
                </label>
              )}

              <div className="flex items-center gap-1.5">
                <input
                  value={testQuery[idx.id] || ""}
                  onChange={(e) => setTestQuery((q) => ({ ...q, [idx.id]: e.target.value }))}
                  placeholder="Query de prueba"
                  className="flex-1 rounded-md border border-zinc-300 bg-white px-2 py-1 text-[11px] outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
                />
                <button
                  onClick={() => test(idx)}
                  disabled={testingId === idx.id}
                  className="rounded-md border border-[var(--accent)] px-2 py-1 text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
                >
                  {testingId === idx.id ? "Probando…" : "Probar"}
                </button>
              </div>
              {result && typeof result === "string" && <p className="text-[10px] text-red-500">{result}</p>}
              {result && Array.isArray(result) && (
                <p className="text-[10px] text-zinc-500">
                  {result.length === 0
                    ? "Sin resultados"
                    : `${result.length} resultado(s) — ${result
                        .slice(0, 3)
                        .map((r) => r.title)
                        .join(", ")}`}
                </p>
              )}
            </li>
          );
        })}
      </ul>

      {modalOpen && <AddIndexerModal onClose={() => setModalOpen(false)} onAdded={refresh} />}
    </div>
  );
}

function AlmacenamientoTab() {
  const [folder, setFolder] = useState<string | null>(null);
  const [uploadKbps, setUploadKbps] = useState("");
  const [downloadKbps, setDownloadKbps] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    Promise.all([api.getLocalLibraryFolder(), api.getSpeedLimits()])
      .then(([f, limits]) => {
        setFolder(f);
        setUploadKbps(limits.upload_kbps?.toString() ?? "");
        setDownloadKbps(limits.download_kbps?.toString() ?? "");
      })
      .finally(() => setLoading(false));
  }, []);

  async function pickFolder() {
    const selected = await open({ directory: true });
    if (!selected || Array.isArray(selected)) return;
    await api.setLocalLibraryFolder(selected);
    setFolder(selected);
  }

  async function saveLimits() {
    setSaving(true);
    setSaved(false);
    try {
      await api.setSpeedLimits(
        uploadKbps.trim() ? Number(uploadKbps) : null,
        downloadKbps.trim() ? Number(downloadKbps) : null,
      );
      setSaved(true);
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return <p className="p-4 pt-0 text-xs text-zinc-500 dark:text-zinc-400">Cargando…</p>;
  }

  return (
    <div className="flex flex-col gap-4 p-4 pt-0">
      <div className="flex items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2.5 dark:border-zinc-800">
        <div className="min-w-0">
          <p className="text-xs font-medium text-zinc-900 dark:text-zinc-100">Carpeta de biblioteca local</p>
          <p className="truncate text-[10px] text-zinc-500">{folder ?? "Sin carpeta configurada"}</p>
        </div>
        <button
          onClick={pickFolder}
          className="shrink-0 rounded-md border border-zinc-300 px-2.5 py-1.5 text-[11px] font-medium text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
        >
          {folder ? "Cambiar" : "Elegir"}
        </button>
      </div>

      <div className="flex flex-col gap-2 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
        <p className="text-xs font-medium text-zinc-900 dark:text-zinc-100">Límite de velocidad</p>
        <div className="flex items-center gap-2">
          <label className="flex flex-1 items-center gap-1.5 text-[11px] text-zinc-600 dark:text-zinc-300">
            Bajada (KB/s)
            <input
              type="number"
              min="0"
              value={downloadKbps}
              onChange={(e) => setDownloadKbps(e.target.value)}
              placeholder="Sin límite"
              className="w-24 rounded-md border border-zinc-300 bg-white px-2 py-1 text-[11px] outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
            />
          </label>
          <label className="flex flex-1 items-center gap-1.5 text-[11px] text-zinc-600 dark:text-zinc-300">
            Subida (KB/s)
            <input
              type="number"
              min="0"
              value={uploadKbps}
              onChange={(e) => setUploadKbps(e.target.value)}
              placeholder="Sin límite"
              className="w-24 rounded-md border border-zinc-300 bg-white px-2 py-1 text-[11px] outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
            />
          </label>
          <button
            onClick={saveLimits}
            disabled={saving}
            className="shrink-0 rounded-md border border-[var(--accent)] px-2.5 py-1.5 text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
          >
            {saving ? "Guardando…" : saved ? "Guardado" : "Guardar"}
          </button>
        </div>
        <p className="text-[10px] text-zinc-500 dark:text-zinc-400">
          Se aplica a las descargas que empiecen desde ahora, no a las que ya están corriendo — el motor de
          torrents no permite reconfigurar en caliente un límite de uno ya agregado.
        </p>
      </div>
    </div>
  );
}

function MotorTab() {
  const [config, setConfig] = useState<TorrentEngineConfig | null>(null);
  const [kind, setKind] = useState<"embedded" | "qbittorrent">("embedded");
  const [baseUrl, setBaseUrl] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<"ok" | string | null>(null);

  function refresh() {
    setLoading(true);
    api
      .getTorrentEngineConfig()
      .then((c) => {
        setConfig(c);
        setKind(c.kind);
        setBaseUrl(c.qbittorrent_base_url ?? "");
        setUsername(c.qbittorrent_username ?? "");
      })
      .finally(() => setLoading(false));
  }

  useEffect(refresh, []);

  async function test() {
    setTesting(true);
    setTestResult(null);
    try {
      await api.testTorrentEngine(baseUrl, username, password);
      setTestResult("ok");
    } catch (e) {
      setTestResult(String(e));
    } finally {
      setTesting(false);
    }
  }

  const canSave = kind === "embedded" || baseUrl.trim().length > 0;

  async function save() {
    if (!canSave) return;
    setSaving(true);
    setSaved(false);
    try {
      await api.setTorrentEngineConfig(
        kind,
        kind === "qbittorrent" ? baseUrl : null,
        kind === "qbittorrent" ? username : null,
        password.trim() ? password : null,
      );
      setPassword("");
      setSaved(true);
      refresh();
    } finally {
      setSaving(false);
    }
  }

  if (loading) {
    return <p className="p-4 pt-0 text-xs text-zinc-500 dark:text-zinc-400">Cargando…</p>;
  }

  return (
    <div className="flex flex-col gap-3 p-4 pt-0">
      <p className="text-xs text-zinc-500 dark:text-zinc-400">
        Por defecto Popcorn descarga con su motor embebido (librqbit), sin nada más que instalar.
        También podés orquestar un qBittorrent que ya tengas corriendo — en ese caso Popcorn nunca
        toca bytes de la red, solo le pide a qBittorrent que agregue/liste/pause/quite torrents vía
        su Web API.
      </p>

      <div className="flex gap-1.5">
        <button
          onClick={() => setKind("embedded")}
          className={`flex-1 rounded-lg border px-3 py-2 text-xs font-medium ${
            kind === "embedded"
              ? "border-[var(--accent)] bg-[var(--accent)] text-white"
              : "border-zinc-300 text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
          }`}
        >
          Embebido (librqbit)
        </button>
        <button
          onClick={() => setKind("qbittorrent")}
          className={`flex-1 rounded-lg border px-3 py-2 text-xs font-medium ${
            kind === "qbittorrent"
              ? "border-[var(--accent)] bg-[var(--accent)] text-white"
              : "border-zinc-300 text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
          }`}
        >
          qBittorrent externo
        </button>
      </div>

      {kind === "qbittorrent" && (
        <div className="flex flex-col gap-2 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
          <div className="rounded-lg border border-zinc-200 p-2.5 text-[11px] text-zinc-500 dark:border-zinc-800 dark:text-zinc-400">
            Popcorn no instala qBittorrent — necesitás tenerlo instalado por tu cuenta con la WebUI
            habilitada (Herramientas → Opciones → Web UI, puerto por defecto 8080).{" "}
            <a
              href="https://www.qbittorrent.org/download"
              target="_blank"
              rel="noreferrer"
              className="font-medium text-[var(--accent)] underline dark:text-[var(--accent-fg)]"
            >
              Descargar qBittorrent
            </a>
            .
          </div>

          <label className="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-300">
            URL de la WebUI
            <input
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="http://127.0.0.1:8080"
              className="rounded-md border border-zinc-300 bg-white px-2 py-1 text-[11px] outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
            />
          </label>
          <label className="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-300">
            Usuario
            <input
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              placeholder="admin"
              className="rounded-md border border-zinc-300 bg-white px-2 py-1 text-[11px] outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
            />
          </label>
          <label className="flex flex-col gap-1 text-[11px] text-zinc-600 dark:text-zinc-300">
            Password
            <input
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder={config?.qbittorrent_has_password ? "•••••••• (ya guardada)" : "Sin guardar"}
              className="rounded-md border border-zinc-300 bg-white px-2 py-1 text-[11px] outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
            />
            <span className="text-[10px] text-zinc-500 dark:text-zinc-400">
              Va directo al keychain del sistema — nunca a la base de datos ni de vuelta al
              frontend. Dejar vacío conserva la que ya esté guardada.
            </span>
          </label>

          <div className="flex items-center gap-2">
            <button
              onClick={test}
              disabled={testing || !baseUrl}
              className="rounded-md border border-[var(--accent)] px-2.5 py-1.5 text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
            >
              {testing ? "Probando…" : "Probar conexión"}
            </button>
            {testResult === "ok" && <span className="text-[11px] text-emerald-600 dark:text-emerald-400">Conectó bien</span>}
            {testResult && testResult !== "ok" && <span className="text-[11px] text-red-500">{testResult}</span>}
          </div>
        </div>
      )}

      <button
        onClick={save}
        disabled={saving || !canSave}
        className="self-start rounded-lg border border-[var(--accent)] px-3 py-1.5 text-xs font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
      >
        {saving ? "Guardando…" : saved ? "Guardado" : "Guardar"}
      </button>
      {!canSave && (
        <p className="text-[10px] text-red-500">Completá la URL de la WebUI antes de guardar.</p>
      )}

      <p className="text-[10px] text-zinc-500 dark:text-zinc-400">
        Si qBittorrent queda configurado pero no se puede conectar al arrancar Popcorn, la app
        igual abre usando el motor embebido y te avisa que no pudo conectar.
      </p>
    </div>
  );
}

type LegalSubTab = "privacidad" | "terminos" | "bittorrent";

const LEGAL_SUB_TABS: { id: LegalSubTab; label: string }[] = [
  { id: "privacidad", label: "Privacidad" },
  { id: "terminos", label: "Términos de uso" },
  { id: "bittorrent", label: "Compartir vía BitTorrent" },
];

function AvisoLegalTab() {
  const [sub, setSub] = useState<LegalSubTab>("privacidad");

  return (
    <div className="flex flex-col gap-3 p-4 pt-0">
      <div className="flex gap-1.5">
        {LEGAL_SUB_TABS.map((t) => (
          <button
            key={t.id}
            onClick={() => setSub(t.id)}
            className={`rounded-lg border px-2.5 py-1.5 text-[11px] font-medium ${
              sub === t.id
                ? "border-[var(--accent)] bg-[var(--accent)] text-white"
                : "border-zinc-300 text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {sub === "privacidad" && <PrivacidadContent />}
      {sub === "terminos" && <TerminosDeUsoContent />}
      {sub === "bittorrent" && (
        <p className="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">{BITTORRENT_SHARING_NOTICE}</p>
      )}
    </div>
  );
}

function LegalSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="space-y-1.5">
      <h3 className="text-[11px] font-semibold uppercase tracking-wide text-[var(--accent)] dark:text-[var(--accent-fg)]">
        {title}
      </h3>
      <div className="text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">{children}</div>
    </section>
  );
}

// Redactado en base a lo que la app hace de verdad hoy (grep del código
// buscando telemetría/analytics/tracking: nada), no una promesa genérica —
// si se agrega algo que cambie estos hechos (ej. un updater con phone-home),
// este texto se actualiza en el mismo cambio.
function PrivacidadContent() {
  return (
    <div className="space-y-3">
      <LegalSection title="Dónde vive tu información">
        <p>
          Popcorn no tiene servidor propio: todo corre en tu máquina. Tu biblioteca, configuración,
          indexers, fuentes IPTV y caché de curación viven en una base de datos SQLite local,
          dentro del directorio de datos de la app en tu sistema — nunca se sincronizan a ningún
          servidor nuestro porque no existe uno.
        </p>
      </LegalSection>

      <LegalSection title="Secretos (API keys, contraseñas)">
        <p>
          Las API keys de proveedores de IA y la contraseña de un motor de torrents externo (ej.
          qBittorrent) se guardan en el keychain del sistema operativo, nunca en la base de datos ni
          en texto plano, y nunca se envían de vuelta a la interfaz una vez guardadas.
        </p>
      </LegalSection>

      <LegalSection title="Qué sale de tu máquina">
        <p>Solo lo que vos mismo configurás o usás activamente:</p>
        <ul className="mt-1 list-inside list-disc space-y-0.5">
          <li>archive.org y Public Domain Torrents (catálogo legal por defecto).</li>
          <li>El proveedor de IA que elijas, con tu propia key (o un Ollama local, sin red).</li>
          <li>Los indexers y fuentes IPTV que agregues por tu cuenta.</li>
          <li>Un qBittorrent externo, si lo configurás en Ajustes → Motor de torrents.</li>
        </ul>
        <p className="mt-1">
          Al descargar por BitTorrent, tu IP es visible para peers/trackers/DHT — es una propiedad
          del protocolo BitTorrent en sí, no algo que Popcorn agregue ni pueda ocultar. No llevamos
          ningún registro de qué archivos transferís: no hay servidor nuestro que pudiera guardarlo,
          y tu propia biblioteca vive solo en tu base de datos local.
        </p>
      </LegalSection>

      <LegalSection title="Lo que no hay">
        <p>
          Sin analytics, sin telemetría, sin reporte de errores a un servidor externo, sin cuenta de
          usuario, sin email ni teléfono, sin login.
        </p>
      </LegalSection>
    </div>
  );
}

function TerminosDeUsoContent() {
  return (
    <div className="space-y-3">
      <LegalSection title="Software provisto tal cual">
        <p>
          Sin garantías de ningún tipo sobre disponibilidad, precisión de metadata o funcionamiento
          ininterrumpido — es software que corrés vos, en tu equipo, bajo tu control.
        </p>
      </LegalSection>

      <LegalSection title="Responsabilidad del contenido">
        <p>
          Sos responsable de lo que agregás, descargás y compartís, y de las fuentes/indexers que
          decidas sumar por tu cuenta. Popcorn no cura ni recomienda ningún indexer de contenido con
          copyright.
        </p>
      </LegalSection>

      <LegalSection title="Uso prohibido">
        <ul className="list-inside list-disc space-y-0.5">
          <li>Contenido de explotación sexual infantil (CSAM) o sexual no consentido, sin excepción.</li>
          <li>Distribución de malware.</li>
        </ul>
      </LegalSection>

      <LegalSection title="Riesgo de lo que descargás">
        <p>
          Popcorn no escanea ni analiza el contenido de los archivos que bajás — los archivos que
          obtengas por torrent o por un indexer que agregaste pueden contener malware. Sos vos quien
          decide qué fuentes agregar y qué descargar; usá tu propio antivirus si te preocupa.
        </p>
      </LegalSection>

      <LegalSection title="Derechos de autor">
        <p>
          Popcorn no aloja contenido — es un cliente que se conecta a fuentes que vos elegís. Si
          creés que algo accedido a través de un indexer o fuente IPTV que agregaste infringe tus
          derechos, el reclamo corresponde a quien opera esa fuente, no a nosotros. Para contenido
          del catálogo por defecto (archive.org), contactá directamente a archive.org.
        </p>
      </LegalSection>

      <LegalSection title="Sin afiliación">
        <p>
          Popcorn no está afiliado a archive.org, qBittorrent, Transmission, ningún proveedor de IA,
          ni a ningún indexer o fuente IPTV que agregues por tu cuenta. Si usás un motor externo o
          traés tu propia API key, también quedás sujeto a los términos de ese tercero.
        </p>
      </LegalSection>

      <LegalSection title="Red social (Nostr) — diseño, todavía no implementada">
        <p>
          Esto describe una decisión de diseño ya cerrada para una fase futura, no un comportamiento
          actual de la app. Cuando exista: Popcorn no opera ningún relé Nostr — te conectás a relés
          de terceros o a uno propio, cada uno con sus propias reglas, igual que elegís un indexer.
          Las comunidades (NIP-72) son moderadas públicamente por quien las crea; toda acción de
          moderación queda firmada y auditable, nunca oculta. Sin mensajes privados (DMs) en la
          primera versión.
        </p>
      </LegalSection>
    </div>
  );
}

interface VentanaTabProps {
  titleBarSide: TitleBarSide;
  onSetTitleBarSide: (side: TitleBarSide) => void;
  titleBarOrder: TitleBarOrder;
  onSetTitleBarOrder: (order: TitleBarOrder) => void;
}

// No hay forma de detectar de qué lado (ni en qué orden) pone tu
// escritorio los controles de ventana — depende del tema/config del WM,
// no del SO. Se deja como preferencia explícita en vez de adivinar.
function VentanaTab({ titleBarSide, onSetTitleBarSide, titleBarOrder, onSetTitleBarOrder }: VentanaTabProps) {
  return (
    <div className="flex flex-col gap-2 p-4 pt-0">
      <div className="flex items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2.5 dark:border-zinc-800">
        <div>
          <p className="text-xs font-medium text-zinc-900 dark:text-zinc-100">Controles de ventana</p>
          <p className="text-[10px] text-zinc-500 dark:text-zinc-400">
            Solo aplica fuera de macOS (ahí siempre va el semáforo a la izquierda).
          </p>
        </div>
        <div className="flex shrink-0 gap-1">
          <button
            onClick={() => onSetTitleBarSide("left")}
            className={`rounded-md border px-2.5 py-1.5 text-[11px] font-medium ${
              titleBarSide === "left"
                ? "border-[var(--accent)] bg-[var(--accent)] text-white"
                : "border-zinc-300 text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
            }`}
          >
            Izquierda
          </button>
          <button
            onClick={() => onSetTitleBarSide("right")}
            className={`rounded-md border px-2.5 py-1.5 text-[11px] font-medium ${
              titleBarSide === "right"
                ? "border-[var(--accent)] bg-[var(--accent)] text-white"
                : "border-zinc-300 text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
            }`}
          >
            Derecha
          </button>
        </div>
      </div>

      <div className="flex items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2.5 dark:border-zinc-800">
        <div>
          <p className="text-xs font-medium text-zinc-900 dark:text-zinc-100">Orden de los botones</p>
          <p className="text-[10px] text-zinc-500 dark:text-zinc-400">Independiente del lado — ajustalo igual.</p>
        </div>
        <div className="flex shrink-0 gap-1">
          <button
            onClick={() => onSetTitleBarOrder("closeFirst")}
            className={`rounded-md border px-2.5 py-1.5 text-[11px] font-medium ${
              titleBarOrder === "closeFirst"
                ? "border-[var(--accent)] bg-[var(--accent)] text-white"
                : "border-zinc-300 text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
            }`}
          >
            Cerrar, minimizar, maximizar
          </button>
          <button
            onClick={() => onSetTitleBarOrder("minimizeFirst")}
            className={`rounded-md border px-2.5 py-1.5 text-[11px] font-medium ${
              titleBarOrder === "minimizeFirst"
                ? "border-[var(--accent)] bg-[var(--accent)] text-white"
                : "border-zinc-300 text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
            }`}
          >
            Minimizar, maximizar, cerrar
          </button>
        </div>
      </div>
    </div>
  );
}
