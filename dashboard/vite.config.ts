import path from "node:path";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv, type Plugin } from "vite";

const projectRoot = path.dirname(fileURLToPath(import.meta.url));

function runtimeConfig(env: Record<string, string | undefined>): Plugin {
	const body = () =>
		JSON.stringify({
			env: {
				API_BASE_URL: env.API_BASE_URL ?? "",
				AUTH0_DOMAIN: env.AUTH0_DOMAIN ?? "",
				AUTH0_CLIENT_ID: env.AUTH0_CLIENT_ID ?? "",
				AUTH0_REDIRECT_URI: env.AUTH0_REDIRECT_URI ?? "",
				AUTH0_AUDIENCE: env.AUTH0_AUDIENCE ?? "",
				DASHBOARD_EXCLUDED_LABELS: env.DASHBOARD_EXCLUDED_LABELS ?? "",
				DEVICE_GRAFANA_URL: env.DEVICE_GRAFANA_URL ?? "",
			},
		});

	const send = (_req: unknown, res: import("node:http").ServerResponse) => {
		res.setHeader("Content-Type", "application/json");
		res.setHeader("Cache-Control", "no-store");
		res.end(body());
	};

	return {
		name: "runtime-config",
		configureServer(server) {
			server.middlewares.use("/config.json", send);
		},
		configurePreviewServer(server) {
			server.middlewares.use("/config.json", send);
		},
	};
}

export default defineConfig(({ mode }) => {
	// Load .env, .env.local, .env.[mode]. Empty prefix = load all keys
	// (Vite's default only exposes VITE_* to the client, but here we just
	// read the values server-side to populate the /config.json middleware).
	const env = { ...process.env, ...loadEnv(mode, projectRoot, "") };

	return {
		plugins: [react(), tailwindcss(), runtimeConfig(env)],
		resolve: {
			alias: {
				"@": projectRoot,
				// Consume the UI package from source so component edits hot-reload
				// without a separate build step. The published dist is only used by
				// external npm consumers.
				"@teton/smith-ui": path.resolve(
					projectRoot,
					"../packages/ui/src/index.ts",
				),
			},
		},
		server: {
			port: 3000,
			host: true,
			// Allow serving the sibling workspace package from the repo root.
			fs: {
				allow: [path.resolve(projectRoot, "..")],
			},
		},
		preview: {
			port: 3000,
			host: true,
		},
	};
});
