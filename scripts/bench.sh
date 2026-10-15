#!/bin/sh
# Benchmark simples Ateliet × Obsidian (macOS), com o mesmo vault aberto:
# tamanho do app, tempo até a janela aparecer, memória (RSS somado de todos os
# processos do app) e CPU depois de alguns segundos parado.
#
# Uso: scripts/bench.sh ~/caminho/do/vault
# Variáveis: ATELIET_APP, OBSIDIAN_APP (caminhos dos .app), WAIT (segundos parado, padrão 15).
# Os dois apps são fechados antes de cada medição.
set -u
export LC_ALL=C  # ponto decimal no printf/awk

VAULT="${1:?uso: scripts/bench.sh <vault>}"
WAIT="${WAIT:-15}"
ATELIET_APP="${ATELIET_APP:-/Applications/Ateliet.app}"
OBSIDIAN_APP="${OBSIDIAN_APP:-/Applications/Obsidian.app}"

now_ms() { perl -MTime::HiRes=time -e 'printf "%d", time * 1000'; }

# Janelas normais (camada 0) visíveis de um processo; não precisa de permissão de acessibilidade.
windows_of() {
  osascript -l JavaScript -e "ObjC.import('CoreGraphics');
    const ws = ObjC.deepUnwrap(ObjC.castRefToObject(\$.CGWindowListCopyWindowInfo(\$.kCGWindowListOptionOnScreenOnly, 0)));
    ws.filter(w => w.kCGWindowOwnerName == '$1' && w.kCGWindowLayer == 0).length" 2>/dev/null || echo 0
}

# O WKWebView do Ateliet roda em processos do WebKit que não são filhos do app:
# conta os que surgirem durante a abertura.
webkit_pids() { pgrep -f 'com.apple.WebKit\.' | sort; }

sum_ps() { # campo, pids…
  field=$1; shift
  [ $# -gt 0 ] || { echo 0; return; }
  ps -o "$field=" -p "$(echo "$@" | tr ' ' ',')" | awk '{ s += $1 } END { printf "%.1f", s }'
}

quit_app() {
  osascript -e "quit app \"$1\"" >/dev/null 2>&1
  sleep 2
  pkill -f "$2/Contents/MacOS/" 2>/dev/null
  sleep 1
}

bench() {
  name=$1 app=$2
  if [ ! -d "$app" ]; then
    printf '%-10s %s não encontrado\n' "$name" "$app"
    return
  fi
  size=$(du -sm "$app" | cut -f1)
  quit_app "$name" "$app"
  before=$(mktemp)
  webkit_pids >"$before"
  t0=$(now_ms)
  if [ "$name" = Ateliet ]; then
    open -a "$app" --args "$VAULT"
  else
    open -a "$app"
    open "obsidian://open?path=$(perl -e '$_ = shift; s/([^A-Za-z0-9._~-])/sprintf("%%%02X", ord $1)/ge; print' "$VAULT")"
  fi
  t_win=""
  while [ $(( $(now_ms) - t0 )) -lt 30000 ]; do
    if [ "$(windows_of "$name")" -gt 0 ]; then t_win=$(( $(now_ms) - t0 )); break; fi
  done
  sleep "$WAIT"
  pids=$(pgrep -f "$app/")
  if [ "$name" = Ateliet ]; then
    pids="$pids $(webkit_pids | comm -13 "$before" -)"
  fi
  rm -f "$before"
  # shellcheck disable=SC2086
  rss=$(sum_ps rss $pids)
  # shellcheck disable=SC2086
  cpu=$(sum_ps %cpu $pids)
  # shellcheck disable=SC2086
  nproc=$(echo $pids | wc -w | tr -d ' ')
  printf '%-10s %8s MB %10s ms %9.0f MB %8s %% %6s\n' "$name" "$size" "${t_win:-?}" "$(echo "$rss / 1024" | bc -l)" "$cpu" "$nproc"
  quit_app "$name" "$app"
}

printf '%-10s %11s %13s %12s %10s %6s\n' "" "app" "janela" "memória" "CPU" "procs"
bench Ateliet "$ATELIET_APP"
bench Obsidian "$OBSIDIAN_APP"
echo
echo "memória = RSS somado (conta páginas compartilhadas mais de uma vez nos dois apps);"
echo "CPU = média recente do ps depois de ${WAIT}s parado; janela = até a janela aparecer, não até o conteúdo."
