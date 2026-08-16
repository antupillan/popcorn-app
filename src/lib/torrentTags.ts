// Parseo determinístico de tags a partir del título crudo de un resultado
// de indexer — nunca IA (Mandato, aclarado en vivo por el usuario: esto
// es texto que ya sigue convenciones regulares, un regex es más rápido y
// confiable que mandarlo a un proveedor). Heurístico sobre texto libre,
// no garantizado: los títulos no siguen un estándar fijo entre indexers,
// así que la ausencia de un tag no significa que el atributo no exista.

const QUALITY_PATTERNS: [RegExp, string][] = [
  [/\b(2160p|4k|uhd)\b/i, "2160p"],
  [/\b1080p\b/i, "1080p"],
  [/\b720p\b/i, "720p"],
  [/\b480p\b/i, "480p"],
];

const SOURCE_PATTERNS: [RegExp, string][] = [
  [/\bblu-?ray\b/i, "BluRay"],
  [/\bbdrip\b/i, "BDRip"],
  [/\bweb-?dl\b/i, "WEB-DL"],
  [/\bwebrip\b/i, "WEBRip"],
  [/\bhdtv\b/i, "HDTV"],
  [/\bdvdrip\b/i, "DVDRip"],
  [/\bcam\b/i, "CAM"],
];

// "Dual Audio"/"DUAL" a propósito NO cuenta como indicador de subtítulos
// — indica pistas de audio en dos idiomas, no que haya subtítulos.
const SUBTITLE_PATTERNS: RegExp[] = [
  /\[\s*es\s*\]/i,
  /\[\s*spa\s*\]/i,
  /\bmulti-?sub\b/i,
  /\bsub\s*espa[ñn]ol\b/i,
  /\besp\b/i,
];

/** Devuelve badges cortos (ej. "1080p", "BDRip", "Subs") a partir del
 * título crudo de un `IndexerResult` — `[]` si no reconoce nada, nunca
 * lanza. */
export function parseTorrentTags(title: string): string[] {
  if (!title) return [];
  const tags: string[] = [];

  for (const [re, label] of QUALITY_PATTERNS) {
    if (re.test(title)) {
      tags.push(label);
      break;
    }
  }
  for (const [re, label] of SOURCE_PATTERNS) {
    if (re.test(title)) {
      tags.push(label);
      break;
    }
  }
  if (SUBTITLE_PATTERNS.some((re) => re.test(title))) {
    tags.push("Subs");
  }

  return tags;
}
