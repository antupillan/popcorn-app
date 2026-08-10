#!/usr/bin/env bash
# tauri dev requiere el puerto fijo 1420 (devUrl en tauri.conf.json); si una
# corrida anterior murió sin limpiar su proceso vite, este puerto queda
# ocupado y "tauri dev" falla con ECONNREFUSED/EADDRINUSE. Este hook libera
# el puerto SOLO si el proceso que lo tiene es un vite/node de este proyecto,
# nunca un proceso arbitrario de otro programa.
set -euo pipefail

PORT=1420
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

PID="$(ss -ltnp 2>/dev/null | grep ":$PORT " | grep -oP 'pid=\K[0-9]+' | head -n1 || true)"
[ -z "$PID" ] && exit 0

CMDLINE="$(tr '\0' ' ' < "/proc/$PID/cmdline" 2>/dev/null || true)"

if [[ "$CMDLINE" == *"vite"* ]] || [[ "$CMDLINE" == *"$PROJECT_DIR"* ]]; then
  echo "predev: liberando puerto $PORT (PID $PID, proceso residual: $CMDLINE)" >&2
  kill "$PID" 2>/dev/null || true
  sleep 0.5
else
  echo "predev: puerto $PORT ocupado por un proceso ajeno (PID $PID: $CMDLINE), no lo toco" >&2
fi
