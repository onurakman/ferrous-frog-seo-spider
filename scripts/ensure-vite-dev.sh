#!/usr/bin/env bash
set -euo pipefail

url="${FERROUS_FROG_DEV_URL:-http://127.0.0.1:1420/}"

if command -v curl >/dev/null 2>&1; then
  if curl -fsS "$url" 2>/dev/null | grep -q "Ferrous Frog SEO Spider"; then
    echo "Ferrous Frog Vite dev server already running at $url"
    exit 0
  fi
fi

exec npm run dev
