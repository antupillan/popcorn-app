import { BITTORRENT_SHARING_NOTICE } from "../lib/legalText";

interface FirstRunScreenProps {
  onAccept: () => void;
}

export function FirstRunScreen({ onAccept }: FirstRunScreenProps) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-zinc-950 p-4 text-zinc-200">
      <div className="my-8 w-full max-w-2xl rounded-xl border border-zinc-800 bg-zinc-900 p-6">
        <h1 className="text-lg font-bold text-white">Bienvenido a Popcorn</h1>
        <p className="mt-1 text-xs text-zinc-400">
          Antes de empezar, esto es lo que necesitás saber sobre cómo funciona esta app.
        </p>

        <section className="mt-5 space-y-2">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-[var(--accent-fg)]">
            Qué es y qué no es
          </h2>
          <p className="text-xs leading-relaxed text-zinc-300">
            Popcorn es software que corrés en tu propia máquina. No hay un servidor central que
            operemos nosotros: tu biblioteca, tus torrents y tu configuración viven en tu equipo.
            No estamos afiliados a archive.org, a ningún operador de relés, ni a qBittorrent,
            Transmission o cualquier indexer que decidas agregar por tu cuenta — cada uno de esos
            servicios es responsabilidad de quien lo opera, no nuestra.
          </p>
        </section>

        <section className="mt-4 space-y-2">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-[var(--accent-fg)]">
            Uso aceptable
          </h2>
          <ul className="list-inside list-disc space-y-1 text-xs leading-relaxed text-zinc-300">
            <li>
              Prohibición absoluta de contenido de explotación sexual infantil (CSAM) o
              contenido sexual no consentido, sin excepción.
            </li>
            <li>Prohibido usar el software para distribuir malware.</li>
            <li>
              Sos responsable de lo que descargás y de las fuentes/indexers que decidas agregar —
              el software no cura ni recomienda ningún indexer de contenido con copyright.
            </li>
          </ul>
        </section>

        <section className="mt-4 space-y-2">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-[var(--accent-fg)]">
            Compartir vía BitTorrent
          </h2>
          <p className="text-xs leading-relaxed text-zinc-300">{BITTORRENT_SHARING_NOTICE}</p>
        </section>

        <section className="mt-4 rounded-lg border border-amber-900/50 bg-amber-950/20 p-3">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-amber-400">
            Si encontraste contenido ilegal
          </h2>
          <p className="mt-1 text-xs leading-relaxed text-zinc-300">
            Este software no es un canal de denuncia ni tiene detección forense certificada de
            CSAM. Si encontraste material de explotación infantil, reportalo directamente a:
          </p>
          <ul className="mt-1.5 space-y-0.5 text-xs text-zinc-300">
            <li>
              <span className="font-semibold">NCMEC CyberTipline</span> —{" "}
              <span className="font-mono text-zinc-400">report.cybertip.org</span>
            </li>
            <li>
              <span className="font-semibold">Red INHOPE</span> (líneas de denuncia por país) —{" "}
              <span className="font-mono text-zinc-400">inhope.org</span>
            </li>
            <li>Autoridades locales de tu jurisdicción.</li>
          </ul>
        </section>

        <button
          onClick={onAccept}
          className="mt-6 w-full rounded-lg bg-[var(--accent)] py-2.5 text-sm font-semibold text-white transition-colors hover:bg-[var(--accent-hover)]"
        >
          Entendido, continuar
        </button>
      </div>
    </div>
  );
}
