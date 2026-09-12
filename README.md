# Popcorn

Centro de medios digitales audiovisuales para escritorio: un cliente P2P
(BitTorrent) que además integra IPTV, un catálogo de YouTube organizado
por temática, y tu propia biblioteca local — todo bajo una sola
biblioteca, con la posibilidad de organizarlo y reproducirlo.

Aplicación nativa (Tauri, no Electron): cada usuario instala y corre su
propio binario. No hay servidor central que operemos nosotros, no hay
cuenta, no hay suscripción.

## Qué incluye

- **Torrents**: motor BitTorrent embebido (`librqbit`) o desacoplable
  hacia tu propio qBittorrent/Transmission ya instalado. Catálogo por
  defecto de licencia explícita (archive.org, Public Domain Torrents) —
  Popcorn nunca trae precargado ni recomienda ningún indexer de
  contenido con copyright; los indexers propios los agrega cada usuario (BYO).
- **IPTV**: listas de canales en vivo agregadas por el usuario.
- **YouTube**: catálogo temático (cine, series, anime) sobre canales
  agregados por el usuario — nunca explora canales fuera de eso.
- **Biblioteca local**: tus propios archivos, organizados junto al resto.
- **Búsqueda global**: filtro instantáneo sobre todo lo ya agregado, más
  búsqueda en profundidad por categoría (torrents/indexers/IPTV/YouTube),
  siempre etiquetada por origen.
- **Subtítulos**: traducción y edición con IA, más integración con
  OpenSubtitles (BYO API key).
- **IA agnóstica de proveedor**: Gemini, OpenAI, Anthropic, Mistral,
  DeepSeek, o un modelo local vía Ollama sin key ni red. Cada quien usa
  su propia clave, guardada en el keychain del sistema operativo.
- **Proxy / VPN** configurable, acotado a alcanzar contenido ya legal
  bloqueado por censura de un estado — nunca para sortear una
  restricción territorial comercial de un licenciante privado.

### En diseño, todavía no implementado

- **Comunidades** (red social vía relés Nostr, protocolo NIP-72):
  identidad local, moderación pública/auditable, sin mensajes directos
  en la primera versión.
- Relé propio auto-hospedable, web-of-trust, ofuscación de tráfico P2P —
  sin diseño cerrado todavía.

## Instalación

Los instaladores para Linux (`.deb`/`.rpm`/`.AppImage`), Windows
(`.msi`/`.exe`) y macOS (`.dmg`, universal Intel + Apple Silicon) se
publican en cada [release de GitHub](https://github.com/antupillan/popcorn-app/releases).

AUR: en preparación, todavía no publicado.

## Compilar desde el código fuente

Requisitos: [Node.js](https://nodejs.org/) 20+, [Rust](https://rustup.rs/)
estable, y en Linux las dependencias de sistema de Tauri (WebKitGTK,
GTK3, etc. — ver [prerequisitos de Tauri](https://v2.tauri.app/start/prerequisites/)).

```bash
npm install
npm run tauri dev    # modo desarrollo, hot-reload
npm run tauri build  # binario + instaladores nativos para tu SO
```

## Motor de torrents

Por defecto Popcorn descarga con `librqbit` embebido (sin dependencias
externas). En Ajustes → Motor de torrents se puede desacoplar hacia un
qBittorrent o Transmission ya instalado — en ese modo Popcorn
no descarga ningún byte, solo orquesta y muestra el estado.

## Agregar indexers propios (BYO)

Popcorn no trae ningún indexer precargado ni recomienda ninguno — cada
usuario aporta su propia plantilla de búsqueda, en Ajustes → Indexers o
directamente desde la lupa → pestaña "Buscadores Torrents".

Para agregar uno hace falta:

1. **Nombre**: cualquiera, solo para identificarlo en la lista.
2. **URL de búsqueda**, con el literal `{query}` donde va el término
   buscado (ej. `https://ejemplo.com/search?q={query}`).
3. **Formato de respuesta**, uno de tres:
   - **Lista de magnets**: la respuesta trae enlaces `magnet:` sueltos —
     Popcorn extrae todos los que encuentra.
   - **RSS**: parsea bloques `<item>`, soporta namespaces (`nyaa:size`,
     etc.) y CDATA. Si el feed no trae un `magnet:` literal pero sí un
     `infoHash`, arma el magnet a partir de ese hash automáticamente.
   - **JSON**: se configuran las rutas dentro de la respuesta —
     "ruta de items" (vacía si la respuesta ya es un array), campo de
     título, campo de magnet, y opcionalmente campo de tamaño y de
     seeders.

Una vez agregado, la pestaña "Buscadores Torrents" de la lupa muestra un
punto de estado por indexer (verde si responde, rojo con el error real
si falla) apenas se entra a esa pestaña.

## Stack técnico

Tauri 2 (Rust + WebView del SO) · React 19 + TypeScript + Vite +
Tailwind CSS · SQLite local (`rusqlite`) · `librqbit` como motor
BitTorrent embebido.

## Legal

Software libre bajo licencia [AGPL-3.0-or-later](LICENSE). Términos de
uso completos en [`terms.html`](terms.html).
