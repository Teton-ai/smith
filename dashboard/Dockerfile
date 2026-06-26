# Build stage — build context is the repo root (npm workspace).
FROM node:22-alpine AS builder

WORKDIR /app

# Install workspace dependencies (root lockfile + workspace manifests) first
# so this layer caches across source-only changes.
COPY package.json package-lock.json ./
COPY dashboard/package.json dashboard/package.json
COPY packages/ui/package.json packages/ui/package.json
RUN npm ci

# Copy sources and build the dashboard. The dashboard's Vite alias resolves
# @teton/smith-ui from packages/ui/src, so no separate package build is needed.
COPY packages/ui packages/ui
COPY dashboard dashboard
RUN npm run build -w @teton/smith-dashboard

# Production stage
FROM nginx:alpine AS runner

COPY --from=builder /app/dashboard/dist /usr/share/nginx/html
COPY dashboard/nginx.conf /etc/nginx/conf.d/default.conf
COPY dashboard/docker-entrypoint.sh /docker-entrypoint.sh
RUN chmod +x /docker-entrypoint.sh && rm /etc/nginx/conf.d/default.conf.bak 2>/dev/null || true

EXPOSE 3000

ENTRYPOINT ["/docker-entrypoint.sh"]
