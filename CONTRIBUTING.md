# Contribuir

Popcorn acepta Issues y Pull Requests, pero con un flujo simple: **todo
Pull Request se revisa a mano antes de fusionarse**. Abrir uno no
garantiza que se acepte, y ninguno se fusiona automáticamente — la rama
`main` está protegida y solo el mantenedor puede aprobar la fusión.

## Antes de abrir un PR

- Para cambios grandes o de diseño, abrir primero un Issue describiendo
  la propuesta — evita trabajo descartado si el enfoque no encaja con
  las decisiones ya cerradas del proyecto (ver más abajo).
- Para bugs chicos o typos, un PR directo está bien.

## Decisiones de diseño ya cerradas (no se reabren en un PR)

- Popcorn nunca lista ni recomienda indexers de contenido con copyright
  — la búsqueda por IA solo interpreta lenguaje natural, cada usuario
  agrega sus propias fuentes (BYO).
- Catálogo legal por defecto: solo fuentes de licencia explícita
  (dominio público / Creative Commons).
- Sin servidor central operado por el proyecto, sin cuenta, sin
  telemetría.
- Cualquier función de IA pasa por el trait `AiProvider` centralizado —
  nunca una llamada aislada a un proveedor específico en otro archivo.

## Requisitos técnicos

- Node.js 20+, Rust estable, dependencias de sistema de Tauri (ver
  [README](README.md#compilar-desde-el-código-fuente)).
- `npx tsc --noEmit` y `cargo test --lib` limpios antes de abrir el PR.
- Tests de Rust colocados junto al módulo que prueban
  (`#[cfg(test)] mod tests` al final del archivo), no en una carpeta
  aparte. Tests que necesiten red real van marcados `#[ignore]`.
- Comentarios técnicos y breves, explican el porqué no obvio (una
  restricción oculta, un workaround), nunca el qué — eso ya lo dice el
  código.

## Reportar bugs de seguridad

No uses un Issue público — ver [SECURITY.md](SECURITY.md).
