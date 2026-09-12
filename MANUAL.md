# Manual de usuario — Popcorn

Guía de cada pantalla de la app. Para instalación, compilación y stack
técnico, ver el [README](README.md).

## Primer uso

Al abrir Popcorn por primera vez aparece una pantalla con la política de
uso aceptable y los recursos de reporte de contenido ilegal — hay que
aceptarla una vez para continuar. El mismo texto queda disponible
después en Ajustes → Aviso legal.

## Biblioteca

La pantalla principal, con pestañas por tipo de contenido:

- **Torrents**: catálogo P2P por defecto (archive.org, Public Domain
  Torrents) — contenido de licencia explícita, curado por la propia app.
  Buscar/agregar indexers propios se hace desde la lupa, no acá.
- **Local**: tus propios archivos. Se elige la carpeta en Ajustes →
  Almacenamiento.
- **IPTV**: canales en vivo, agregados con el botón **"+"** de la barra
  superior.
- **YouTube**: catálogo por temática (Cine, Series, Anime, Música) sobre
  canales que agregaste — nunca explora canales fuera de eso. Tiene dos
  sub-pestañas: **Videos** (lo que se puede reproducir, filtrable por
  categoría) y **Fuentes** (gestionar los canales agregados). Si un
  video no carga por falta de API key configurada, aparece un botón
  "Reintentar" — no hace falta cerrar y reabrir la pestaña.
- **Mi Colección**: todo lo que ya agregaste, de cualquier fuente.

## Búsqueda global (lupa)

Ícono de lupa en la barra superior. Pestañas:

- **Todo**: filtro instantáneo sobre todo lo ya agregado, agrupado por
  origen. Botón "Buscar con IA" rankea por significado, no solo texto.
- **Torrents**: el catálogo Online ya cargado, más "Buscar en
  archive.org" para búsqueda en vivo más allá de lo curado.
- **Buscadores Torrents**: tus indexers propios (BYO). Apenas se entra
  a esta pestaña, cada indexer muestra un punto de estado (gris
  mientras chequea, verde si responde, rojo con el error real si
  falla). Hay que escribir un término y presionar **Enter** o el botón
  "Buscar en mis indexers" — no busca solo mientras se escribe, es una
  acción explícita.
- **IPTV**: filtro instantáneo sobre tus canales agregados.
- **YouTube**: filtro instantáneo sobre lo ya cargado, más "Buscar más
  allá de lo cargado" para paginar más adentro de tus canales ya
  agregados (nunca explora canales nuevos).

Enter dispara la acción de búsqueda en profundidad de la pestaña activa
en todas las categorías, no solo "Todo".

## Subtítulos

Sección propia en la barra lateral. Permite traducir y editar
subtítulos con IA (respeta los timestamps del SRT/VTT original), y
buscar/descargar desde OpenSubtitles si configuraste esa integración en
Ajustes.

## Ajustes

Panel con secciones en acordeón:

- **Ayuda**: instrucciones para conseguir API keys de YouTube, Gemini,
  OpenAI/DeepSeek/Mistral, y este manual.
- **IA**: agregar proveedores (Gemini, OpenAI, Anthropic, Mistral,
  DeepSeek, u Ollama local sin key). Cada proveedor usa tu propia clave,
  guardada en el keychain del sistema operativo.
- **Indexers**: agregar/editar/quitar tus indexers BYO de torrents (ver
  sección de indexers en el [README](README.md#agregar-indexers-propios-byo)
  para el detalle de los 3 formatos soportados).
- **Almacenamiento**: carpeta de Biblioteca Local, límites de velocidad
  de subida/bajada.
- **Ventana**: tema (claro/oscuro/sistema), lado y orden de los
  controles de la barra de título, efectos nativos (Mica en Windows,
  vibrancy en macOS — no aplica en Linux).
- **Motor de torrents**: elegir entre el motor embebido (`librqbit`) o
  desacoplar hacia un qBittorrent/Transmission ya instalado.
- **Proxy / VPN**: configurar un proxy para el motor de torrents y/o
  para las llamadas de catálogo — pensado para censura real de un
  estado, no para sortear un bloqueo territorial comercial.
- **YouTube**: pegar tu YouTube Data API key.
- **OpenSubtitles**: configurar cuenta/API key propia para buscar y
  descargar subtítulos de esa plataforma.
- **Comunidades** y **Avanzado**: marcadas "próximamente" — la capa
  social vía Nostr todavía no está implementada.
- **Aviso legal**: privacidad, términos de uso, y el aviso de qué
  implica compartir vía BitTorrent — mismo texto que la pantalla de
  primer uso, disponible en cualquier momento.

## Reproducción

Al reproducir algo (torrent, canal IPTV, video de YouTube, archivo
local), el reproductor sigue sonando/reproduciendo aunque se minimice la
ventana o se achique — no hace falta un reproductor aparte para dejar
algo de fondo mientras se usa el resto de la app.

Si un torrent sembrado automáticamente llega a ratio 1:1 (se subió tanto
como se bajó), aparece un aviso único preguntando si se quiere seguir
sembrando o dejar de hacerlo.
