# Seguridad

Popcorn es software libre mantenido por una sola persona, sin empresa
operadora detrás. No hay servidor propio ni datos de usuarios que
administremos — el alcance de seguridad relevante es el propio binario:
el motor de torrents embebido, el manejo de API keys en el keychain del
sistema operativo, y el candado de publicación pre-comunidad.

## Cómo reportar una vulnerabilidad

Dos canales, según preferencia:

1. **Private vulnerability reporting de GitHub** (recomendado): pestaña
   [Security](https://github.com/antupillan/popcorn-app/security) de
   este repositorio → "Report a vulnerability". Queda privado hasta que
   se resuelva.
2. **Email**: `168942727+antupillan@users.noreply.github.com`.

No usar Issues públicos para reportar una vulnerabilidad sin parchear
todavía.

## Qué NO es una vulnerabilidad de Popcorn

- Contenido accedido a través de un indexer, fuente IPTV o canal de
  YouTube que el propio usuario agregó — la responsabilidad de esas
  fuentes es de quien las opera, no del proyecto.
- Exposición de la IP del usuario ante peers/trackers/DHT al usar
  BitTorrent — es una propiedad del protocolo en sí, no un bug de
  Popcorn.

## Alcance

- Motor de torrents embebido (`librqbit`) y su integración.
- Manejo de secretos (API keys, credenciales de motores externos) —
  deben vivir solo en el keychain del sistema operativo, nunca en la
  base de datos SQLite local ni en texto plano.
- El candado de publicación pre-comunidad (fail-closed) — cualquier
  bypass de ese chequeo es una vulnerabilidad seria.
