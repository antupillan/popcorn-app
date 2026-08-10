import { useState } from "react";
import { api } from "../lib/api";

interface AddAiProviderModalProps {
  onClose: () => void;
  onAdded: () => void;
}

type Kind = "gemini" | "openai_compatible";

// Presets de conveniencia — solo precargan el campo de texto, el usuario
// puede escribir cualquier otra URL (Mandato Zero Hardcode: esto no ata
// ningún comportamiento a un proveedor fijo, es azúcar de UI).
const BASE_URL_PRESETS: { label: string; url: string }[] = [
  { label: "OpenAI", url: "https://api.openai.com/v1" },
  { label: "DeepSeek", url: "https://api.deepseek.com/v1" },
  { label: "Mistral", url: "https://api.mistral.ai/v1" },
  { label: "Ollama local", url: "http://localhost:11434/v1" },
];

export function AddAiProviderModal({ onClose, onAdded }: AddAiProviderModalProps) {
  const [kind, setKind] = useState<Kind>("gemini");
  const [label, setLabel] = useState("");
  const [model, setModel] = useState("");
  const [baseUrl, setBaseUrl] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function add() {
    if (!label.trim() || !model.trim()) return;
    setLoading(true);
    setError(null);
    try {
      await api.addAiProvider(
        kind,
        label.trim(),
        model.trim(),
        kind === "openai_compatible" ? baseUrl.trim() || null : null,
        apiKey.trim() || null,
      );
      onAdded();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4">
      <div className="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl border border-zinc-200 bg-white shadow-xl dark:border-zinc-800 dark:bg-zinc-900">
        <div className="flex items-center justify-between border-b border-zinc-200 px-4 py-3 dark:border-zinc-800">
          <h2 className="text-sm font-semibold text-zinc-900 dark:text-white">Agregar proveedor IA</h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" className="h-4 w-4">
              <path d="M18 6 6 18M6 6l12 12" />
            </svg>
          </button>
        </div>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            add();
          }}
          className="flex flex-1 flex-col gap-3 overflow-y-auto p-4"
        >
          <p className="text-xs text-zinc-500 dark:text-zinc-400">
            La API key va directo al keychain del sistema — nunca a la base de datos ni de vuelta al frontend.
          </p>

          <select
            value={kind}
            onChange={(e) => setKind(e.target.value as Kind)}
            className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
          >
            <option value="gemini">Gemini</option>
            <option value="openai_compatible">OpenAI-compatible (OpenAI/DeepSeek/Mistral/Ollama)</option>
          </select>

          <input
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder="Nombre para identificarlo (ej. Gemini principal)"
            className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
          />

          <input
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder={kind === "gemini" ? "ej. gemini-2.0-flash" : "ej. gpt-4o-mini"}
            className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
          />

          {kind === "openai_compatible" && (
            <>
              <div className="flex flex-wrap gap-1.5">
                {BASE_URL_PRESETS.map((p) => (
                  <button
                    key={p.label}
                    type="button"
                    onClick={() => setBaseUrl(p.url)}
                    className="rounded-md border border-zinc-300 px-2 py-1 text-[11px] text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
                  >
                    {p.label}
                  </button>
                ))}
              </div>
              <input
                value={baseUrl}
                onChange={(e) => setBaseUrl(e.target.value)}
                placeholder="Base URL (ej. https://api.openai.com/v1)"
                className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
              />
            </>
          )}

          <input
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder="API key (opcional para Ollama local)"
            className="rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-xs outline-none focus:border-[var(--accent-hover)] dark:border-zinc-700 dark:bg-zinc-950"
          />

          {error && <p className="text-xs text-red-500">{error}</p>}

          <button
            type="submit"
            disabled={loading}
            className="self-end rounded-lg bg-[var(--accent)] px-3 py-1.5 text-xs font-semibold text-white hover:bg-[var(--accent-hover)] disabled:opacity-50"
          >
            {loading ? "Agregando…" : "Agregar"}
          </button>
        </form>
      </div>
    </div>
  );
}
