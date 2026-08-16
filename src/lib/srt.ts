// Parseo/serializado de subtítulos SRT — vive enteramente en el frontend
// (el backend solo persiste el blob de texto, ver src-tauri/src/subtitles.rs).
// Port verificado del `shiftTime` real de POPCORN.NETVERSE/src/components/
// SubtitleStudio.tsx (prototipo descartado) — esa parte era lógica genuina,
// no simulada; se descartó todo lo demás de ese archivo (datos mock,
// sistema de estrellas, donación fabricada).

export interface Cue {
  id: string;
  startMs: number;
  endMs: number;
  text: string;
}

function timeToMs(time: string): number {
  const m = time.match(/(\d{2}):(\d{2}):(\d{2})[,.](\d{3})/);
  if (!m) return 0;
  const [, h, min, s, ms] = m;
  return Number(h) * 3600000 + Number(min) * 60000 + Number(s) * 1000 + Number(ms);
}

function msToTime(totalMs: number, separator: "," | "."): string {
  const clamped = Math.max(0, Math.round(totalMs));
  const h = Math.floor(clamped / 3600000);
  const m = Math.floor((clamped % 3600000) / 60000);
  const s = Math.floor((clamped % 60000) / 1000);
  const ms = clamped % 1000;
  const pad = (n: number, len = 2) => String(n).padStart(len, "0");
  return `${pad(h)}:${pad(m)}:${pad(s)}${separator}${pad(ms, 3)}`;
}

// Bloques separados por línea(s) en blanco: índice, línea de tiempo
// "HH:MM:SS,mmm --> HH:MM:SS,mmm", una o más líneas de texto. El número de
// índice del archivo original se ignora — se renumera al serializar,
// mismo criterio que cualquier editor de subtítulos real (el índice no es
// información, es solo una etiqueta secuencial).
export function parseSrt(content: string): Cue[] {
  const normalized = content.replace(/\r\n/g, "\n").trim();
  if (!normalized) return [];
  const blocks = normalized.split(/\n\s*\n/);
  const cues: Cue[] = [];
  for (const block of blocks) {
    const lines = block.split("\n").filter((l) => l.trim().length > 0);
    if (lines.length < 2) continue;
    const timeLineIdx = lines.findIndex((l) => l.includes("-->"));
    if (timeLineIdx === -1) continue;
    const [startRaw, endRaw] = lines[timeLineIdx].split("-->").map((s) => s.trim());
    const text = lines.slice(timeLineIdx + 1).join("\n");
    cues.push({
      id: crypto.randomUUID(),
      startMs: timeToMs(startRaw),
      endMs: timeToMs(endRaw),
      text,
    });
  }
  return cues;
}

export function serializeSrt(cues: Cue[]): string {
  return cues
    .map((c, i) => `${i + 1}\n${msToTime(c.startMs, ",")} --> ${msToTime(c.endMs, ",")}\n${c.text}`)
    .join("\n\n");
}

// Offset en ms aplicado a start/end de cada cue, saturando en 0 (nunca
// timestamps negativos) — mismo comportamiento que el `shiftTime` del
// prototipo, ahí ya estaba bien resuelto.
export function shiftCueTimestamps(cues: Cue[], offsetMs: number): Cue[] {
  return cues.map((c) => ({
    ...c,
    startMs: Math.max(0, c.startMs + offsetMs),
    endMs: Math.max(0, c.endMs + offsetMs),
  }));
}

// SRT -> WebVTT: header obligatorio + separador decimal `.` en vez de `,`
// (única diferencia real de sintaxis para cues simples sin posicionamiento).
// Usado para el <track> del <video> en VideoPlayer.tsx — WebVTT es el único
// formato de subtítulos que el HTML5 <track> soporta nativamente, los
// navegadores no entienden SRT crudo.
export function srtToVtt(content: string): string {
  const cues = parseSrt(content);
  const body = cues.map((c) => `${msToTime(c.startMs, ".")} --> ${msToTime(c.endMs, ".")}\n${c.text}`).join("\n\n");
  return `WEBVTT\n\n${body}`;
}
