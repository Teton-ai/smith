import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import dts from "vite-plugin-dts";

export default defineConfig({
	plugins: [react(), dts({ include: ["src"], insertTypesEntry: true })],
	build: {
		lib: {
			entry: fileURLToPath(new URL("./src/index.ts", import.meta.url)),
			formats: ["es"],
			fileName: "index",
		},
		rollupOptions: {
			// Consumers provide these; never bundle them into the library.
			external: [
				"react",
				"react-dom",
				"react/jsx-runtime",
				"react-router",
				"lucide-react",
			],
		},
	},
});
