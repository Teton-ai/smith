#!/bin/sh
set -eu

# Generate runtime config from environment variables. The dashboard fetches
# /config.json on boot; we rewrite it here so the same image works in any env.
cat > /usr/share/nginx/html/config.json <<EOF
{
	"env": {
		"API_BASE_URL": "${API_BASE_URL:-}",
		"AUTH0_DOMAIN": "${AUTH0_DOMAIN:-}",
		"AUTH0_CLIENT_ID": "${AUTH0_CLIENT_ID:-}",
		"AUTH0_REDIRECT_URI": "${AUTH0_REDIRECT_URI:-}",
		"AUTH0_AUDIENCE": "${AUTH0_AUDIENCE:-}",
		"DASHBOARD_EXCLUDED_LABELS": "${DASHBOARD_EXCLUDED_LABELS:-}",
		"DEVICE_GRAFANA_URL": "${DEVICE_GRAFANA_URL:-}"
	}
}
EOF

exec nginx -g 'daemon off;'
