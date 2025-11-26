#!/bin/bash
set -e

MODE="${1:-${RUN_MODE:-all}}"

echo "🚀 Starting BacktestingEngine in mode: $MODE"

case "$MODE" in
  all)
    echo "📦 Starting ALL services: nginx (UI) + API server + workers"
    # Start supervisord with all services (uses default supervisord.conf)
    exec /usr/bin/supervisord -c /etc/supervisord.conf
    ;;
  
  api)
    echo "🌐 Starting API ONLY (nginx + API server, no background workers)"
    # Create custom supervisord config with --workers 0
    cat > /tmp/supervisord-api.conf <<'EOF'
[supervisord]
nodaemon=true
logfile=/dev/stdout
logfile_maxbytes=0
loglevel=info

[program:nginx]
command=/usr/sbin/nginx -g 'daemon off;'
autostart=true
autorestart=true
stdout_logfile=/dev/stdout
stdout_logfile_maxbytes=0
stderr_logfile=/dev/stderr
stderr_logfile_maxbytes=0

[program:backtesting-server]
command=/bin/server serve --workers 0
autostart=true
autorestart=true
stdout_logfile=/dev/stdout
stdout_logfile_maxbytes=0
stderr_logfile=/dev/stderr
stderr_logfile_maxbytes=0
environment=RUST_LOG=info
EOF
    exec /usr/bin/supervisord -c /tmp/supervisord-api.conf
    ;;
  
  worker)
    echo "⚙️  Starting WORKER ONLY (no UI, no API)"
    # Run workers directly without supervisord or nginx
    exec /bin/server serve
    ;;
  
  *)
    echo "❌ Invalid mode: $MODE"
    echo "Valid modes: all, api, worker"
    exit 1
    ;;
esac
