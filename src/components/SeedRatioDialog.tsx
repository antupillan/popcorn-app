interface SeedRatioDialogProps {
  torrentName: string;
  onKeep: () => void;
  onRemove: () => void;
}

// Aviso único al llegar a relación 1:1 en un torrent sembrado
// automáticamente (ver App.tsx) — Sí quita el archivo, No lo deja
// sembrando indefinido y no se vuelve a preguntar (localStorage).
export function SeedRatioDialog({ torrentName, onKeep, onRemove }: SeedRatioDialogProps) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
      <div className="w-full max-w-sm rounded-xl border border-zinc-200/50 bg-white/80 p-5 shadow-2xl backdrop-blur-xl dark:border-zinc-800/50 dark:bg-zinc-900/80">
        <h2 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100">Sembrado 1:1 completo</h2>
        <p className="mt-1.5 truncate text-xs text-zinc-500 dark:text-zinc-400">{torrentName}</p>
        <p className="mt-2 text-xs leading-relaxed text-zinc-600 dark:text-zinc-300">
          Subiste tanto como bajaste — retribuiste al swarm. ¿Querés quitar el archivo?
        </p>
        <div className="mt-4 flex gap-2">
          <button
            onClick={onKeep}
            className="flex-1 rounded-md border border-zinc-300 py-1.5 text-xs font-medium text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
          >
            No, seguir sembrando
          </button>
          <button
            onClick={onRemove}
            className="flex-1 rounded-md bg-red-600 py-1.5 text-xs font-semibold text-white hover:bg-red-500"
          >
            Sí, quitar
          </button>
        </div>
      </div>
    </div>
  );
}
