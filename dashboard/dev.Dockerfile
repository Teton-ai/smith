# Development Dockerfile for the dashboard with hot reloading.
# Build context is the repo root (npm workspace).
FROM node:22-alpine

WORKDIR /app

# Install workspace dependencies first for better caching.
COPY package.json package-lock.json ./
COPY dashboard/package.json dashboard/package.json
COPY packages/ui/package.json packages/ui/package.json
RUN npm i

# Copy source code (compose mounts override these for live reload).
COPY packages/ui packages/ui
COPY dashboard dashboard

EXPOSE 3000
ENV NODE_ENV=development

# Run the dashboard dev server; Vite serves @teton/smith-ui from source.
CMD ["npm", "run", "dev", "-w", "@teton/smith-dashboard"]
