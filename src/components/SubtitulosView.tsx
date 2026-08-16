import { useEffect, useState } from "react";
import type { ChangeEvent } from "react";
import { api } from "../lib/api";
import type { MediaItem, LocalFile, Subtitle } from "../types";
import { parseSrt, serializeSrt, shiftCueTimestamps } from "../lib/srt";
import type { Cue } from "../lib/srt";

type ContentKind = "media" | "local";

interface ContentItem {
  kind: ContentKind;
  id: string; // media_items.id o el path del archivo local — media_item_id
  // en el backend es un identificador genérico, sin FK real (mismo
  // criterio de acoplamiento flojo que source_settings.id).
  title: string;
}

const ORIGIN_LABEL: Record<Subtitle["origin"], string> = {
  original: "Original",
  ai_translated: "Traducido por IA",
  human_edited: "Editado",
};

// Vista de primer nivel del Sidebar — Parte A del plan de subtítulos
// (traducir + editar localmente, ver Planes_mejora_popcorn/subtitulos_ia.txt).
// Solo Mi Colección y Biblioteca Local: ambos con duración fija,
// reproducidos por el mismo <video> propio — YouTube (iframe cross-origin)
// e IPTV en vivo (sin timeline fijo) quedan fuera por límite técnico real.
export function SubtitulosView() {
  const [items, setItems] = useState<ContentItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<ContentItem | null>(null);

  useEffect(() => {
    setLoading(true);
    Promise.all([api.listMediaItems(), api.listLocalFiles()])
      .then(([media, local]: [MediaItem[], LocalFile[]]) => {
        const combined: ContentItem[] = [
          ...media.map((m) => ({ kind: "media" as const, id: m.id, title: m.title })),
          ...local.map((f) => ({ kind: "local" as const, id: f.path, title: f.name })),
        ];
        setItems(combined);
      })
      .finally(() => setLoading(false));
  }, []);

  return (
    <div className="flex h-full overflow-hidden">
      <div className="w-64 shrink-0 overflow-y-auto border-r border-zinc-200 dark:border-zinc-800">
        <p className="px-3 pt-3 text-[10px] font-semibold uppercase tracking-wide text-zinc-400">
          Mi Colección + Local
        </p>
        {loading && <p className="p-3 text-xs text-zinc-500 dark:text-zinc-400">Cargando…</p>}
        {!loading && items.length === 0 && (
          <p className="p-3 text-xs text-zinc-500 dark:text-zinc-400">
            No hay contenido agregado todavía en Mi Colección ni en Local.
          </p>
        )}
        <ul className="flex flex-col gap-0.5 p-2">
          {items.map((item) => (
            <li key={`${item.kind}:${item.id}`}>
              <button
                onClick={() => setSelected(item)}
                className={`flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-left text-xs ${
                  selected?.kind === item.kind && selected?.id === item.id
                    ? "bg-[var(--accent-soft)] text-[var(--accent)] dark:text-[var(--accent-fg)]"
                    : "text-zinc-700 hover:bg-zinc-100 dark:text-zinc-300 dark:hover:bg-zinc-800"
                }`}
              >
                <span className="truncate">{item.title}</span>
              </button>
            </li>
          ))}
        </ul>
      </div>

      <div className="flex-1 overflow-y-auto">
        {!selected ? (
          <p className="p-6 text-center text-xs text-zinc-500 dark:text-zinc-400">
            Elegí un ítem de la izquierda para ver o agregar subtítulos.
          </p>
        ) : (
          <SubtitlesPanel key={`${selected.kind}:${selected.id}`} item={selected} />
        )}
      </div>
    </div>
  );
}

function SubtitlesPanel({ item }: { item: ContentItem }) {
  const [subtitles, setSubtitles] = useState<Subtitle[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [pasteText, setPasteText] = useState("");
  const [pasteLanguage, setPasteLanguage] = useState("es");
  const [editing, setEditing] = useState<Subtitle | null>(null);

  function refresh() {
    setLoading(true);
    api
      .listSubtitles(item.id)
      .then(setSubtitles)
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }

  useEffect(refresh, [item.id]);

  async function addOriginal() {
    if (!pasteText.trim()) return;
    setError(null);
    try {
      await api.addSubtitleText(item.id, pasteLanguage.trim() || "es", "original", pasteText);
      setPasteText("");
      refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleFile(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    const text = await file.text();
    setError(null);
    try {
      await api.addSubtitleText(item.id, pasteLanguage.trim() || "es", "original", text);
      refresh();
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <div className="flex flex-col gap-4 p-4">
      <h2 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">{item.title}</h2>

      <div className="flex flex-col gap-2 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
        <p className="text-xs font-medium text-zinc-900 dark:text-zinc-100">Agregar subtítulo original</p>
        <label className="flex items-center gap-1.5 text-[11px] text-zinc-600 dark:text-zinc-300">
          Idioma (código, ej. es/en/pt)
          <input
            value={pasteLanguage}
            onChange={(e) => setPasteLanguage(e.target.value)}
            className="w-20 rounded-md border border-zinc-300 bg-white px-2 py-1 text-[11px] outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
          />
        </label>
        <textarea
          value={pasteText}
          onChange={(e) => setPasteText(e.target.value)}
          placeholder={"1\n00:00:02,100 --> 00:00:05,400\nTexto del subtítulo…"}
          rows={4}
          className="rounded-lg border border-zinc-300 bg-white px-3 py-2 font-mono text-[11px] outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
        />
        <div className="flex items-center gap-2">
          <button
            onClick={addOriginal}
            disabled={!pasteText.trim()}
            className="rounded-md border border-[var(--accent)] px-2.5 py-1 text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
          >
            Pegar como subtítulo
          </button>
          <label className="cursor-pointer rounded-md border border-zinc-300 px-2.5 py-1 text-[11px] text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800">
            Subir archivo .srt
            <input type="file" accept=".srt" className="hidden" onChange={handleFile} />
          </label>
        </div>
      </div>

      {error && <p className="text-xs text-red-500">{error}</p>}
      {loading && <p className="text-xs text-zinc-500 dark:text-zinc-400">Cargando subtítulos…</p>}
      {!loading && subtitles.length === 0 && (
        <p className="text-xs text-zinc-500 dark:text-zinc-400">Sin subtítulos todavía para este ítem.</p>
      )}

      <ul className="flex flex-col gap-1.5">
        {subtitles.map((s) => (
          <li key={s.id} className="flex items-center justify-between gap-2 rounded-lg border border-zinc-200 px-3 py-2 dark:border-zinc-800">
            <div className="min-w-0">
              <p className="truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">{s.language}</p>
              <span className="rounded-full bg-[var(--accent-soft)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--accent)] dark:text-[var(--accent-fg)]">
                {ORIGIN_LABEL[s.origin]}
              </span>
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              <button
                onClick={() => setEditing(s)}
                className="rounded-md border border-zinc-300 px-2 py-1 text-[11px] text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
              >
                Abrir/Editar
              </button>
              <button
                onClick={() => api.removeSubtitle(s.id).then(refresh)}
                className="rounded-md border border-red-300 px-2 py-1 text-[11px] text-red-600 hover:bg-red-50 dark:border-red-900 dark:text-red-400 dark:hover:bg-red-950/40"
              >
                Quitar
              </button>
            </div>
          </li>
        ))}
      </ul>

      {editing && (
        <CueEditor
          mediaItemId={item.id}
          subtitle={editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            refresh();
          }}
        />
      )}
    </div>
  );
}

interface CueEditorProps {
  mediaItemId: string;
  subtitle: Subtitle;
  onClose: () => void;
  onSaved: () => void;
}

function CueEditor({ mediaItemId, subtitle, onClose, onSaved }: CueEditorProps) {
  const [cues, setCues] = useState<Cue[]>(() => parseSrt(subtitle.content));
  const [offsetMs, setOffsetMs] = useState(0);
  const [targetLang, setTargetLang] = useState("en");
  const [translating, setTranslating] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Provenance real: si lo único que pasó desde el original fue una
  // traducción (más ajuste de offset, que no toca el texto), la versión
  // sigue siendo "de la IA" — un edit manual de texto la vuelve
  // "human_edited" y ya no puede volver atrás en la misma sesión.
  const [wasTranslated, setWasTranslated] = useState(false);

  function applyOffset(delta: number) {
    setOffsetMs((o) => o + delta);
    setCues((prev) => shiftCueTimestamps(prev, delta));
  }

  function editCueText(id: string, text: string) {
    setCues((prev) => prev.map((c) => (c.id === id ? { ...c, text } : c)));
    setWasTranslated(false);
  }

  async function translate() {
    if (!targetLang.trim()) return;
    setTranslating(true);
    setError(null);
    try {
      const translated = await api.translateSubtitleTexts(cues.map((c) => c.text), targetLang.trim());
      if (translated.length !== cues.length) {
        throw new Error("el proveedor devolvió una cantidad de líneas distinta a la esperada");
      }
      setCues((prev) => prev.map((c, i) => ({ ...c, text: translated[i] })));
      setWasTranslated(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setTranslating(false);
    }
  }

  async function save() {
    setSaving(true);
    setError(null);
    const origin = wasTranslated ? "ai_translated" : "human_edited";
    const language = wasTranslated ? targetLang.trim() || subtitle.language : subtitle.language;
    try {
      await api.addSubtitleText(mediaItemId, language, origin, serializeSrt(cues));
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4" onClick={onClose}>
      <div
        onClick={(e) => e.stopPropagation()}
        className="flex max-h-[85vh] w-full max-w-2xl flex-col rounded-xl border border-zinc-200 bg-white shadow-xl dark:border-zinc-800 dark:bg-zinc-900"
      >
        <div className="flex items-center justify-between border-b border-zinc-200 px-4 py-3 dark:border-zinc-800">
          <h2 className="text-sm font-semibold text-zinc-900 dark:text-white">
            Editar subtítulo ({subtitle.language}, {ORIGIN_LABEL[subtitle.origin]})
          </h2>
          <button onClick={onClose} className="rounded-md p-1 text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="flex flex-col gap-3 overflow-y-auto p-4">
          <div className="flex flex-wrap items-center gap-2 rounded-lg border border-zinc-200 p-2.5 dark:border-zinc-800">
            <span className="text-[11px] font-medium text-zinc-600 dark:text-zinc-300">Sincronización:</span>
            <button onClick={() => applyOffset(-500)} className="rounded-md bg-zinc-100 px-2 py-1 text-[11px] dark:bg-zinc-800 dark:text-zinc-200">
              -500 ms
            </button>
            <button onClick={() => applyOffset(-100)} className="rounded-md bg-zinc-100 px-2 py-1 text-[11px] dark:bg-zinc-800 dark:text-zinc-200">
              -100 ms
            </button>
            <button onClick={() => applyOffset(100)} className="rounded-md bg-zinc-100 px-2 py-1 text-[11px] dark:bg-zinc-800 dark:text-zinc-200">
              +100 ms
            </button>
            <button onClick={() => applyOffset(500)} className="rounded-md bg-zinc-100 px-2 py-1 text-[11px] dark:bg-zinc-800 dark:text-zinc-200">
              +500 ms
            </button>
            <span className="ml-auto font-mono text-[11px] text-[var(--accent)] dark:text-[var(--accent-fg)]">
              {offsetMs > 0 ? `+${offsetMs}` : offsetMs} ms
            </span>
          </div>

          <div className="flex flex-wrap items-center gap-2 rounded-lg border border-zinc-200 p-2.5 dark:border-zinc-800">
            <span className="text-[11px] font-medium text-zinc-600 dark:text-zinc-300">Traducir a:</span>
            <input
              value={targetLang}
              onChange={(e) => setTargetLang(e.target.value)}
              placeholder="en"
              className="w-16 rounded-md border border-zinc-300 bg-white px-2 py-1 text-[11px] outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
            />
            <button
              onClick={translate}
              disabled={translating}
              className="rounded-md border border-[var(--accent)] px-2.5 py-1 text-[11px] font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
            >
              {translating ? "Traduciendo…" : "Traducir con IA"}
            </button>
          </div>

          {error && <p className="text-xs text-red-500">{error}</p>}

          <div className="flex flex-col gap-2 max-h-72 overflow-y-auto pr-1">
            {cues.map((c, i) => (
              <div key={c.id} className="rounded-lg border border-zinc-200 p-2 dark:border-zinc-800">
                <div className="flex items-center justify-between font-mono text-[10px] text-zinc-500">
                  <span>#{i + 1}</span>
                  <span>
                    {(c.startMs / 1000).toFixed(3)}s → {(c.endMs / 1000).toFixed(3)}s
                  </span>
                </div>
                <textarea
                  value={c.text}
                  onChange={(e) => editCueText(c.id, e.target.value)}
                  rows={2}
                  className="mt-1 w-full resize-none bg-transparent text-xs text-zinc-900 outline-none dark:text-zinc-100"
                />
              </div>
            ))}
          </div>

          <div className="flex items-center gap-2 self-end">
            <button
              onClick={save}
              disabled={saving}
              className="rounded-lg border border-[var(--accent)] px-3 py-1.5 text-xs font-semibold text-[var(--accent)] hover:bg-[var(--accent)] hover:text-white disabled:opacity-50 dark:text-[var(--accent-fg)]"
            >
              {saving ? "Guardando…" : "Guardar como versión nueva"}
            </button>
          </div>
          <p className="text-[10px] text-zinc-500 dark:text-zinc-400">
            Guardar crea una versión nueva — el subtítulo original queda intacto, nunca se sobrescribe.
          </p>
        </div>
      </div>
    </div>
  );
}
