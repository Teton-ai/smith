"use client";

import { useEffect } from "react";

const RELOAD_KEY = "__chunkErrorReloadAt";
const RELOAD_COOLDOWN_MS = 30_000;

// During a deploy, the running tab still references the previous build's
// hashed chunk filenames. The new ECS task only ships the new build's
// filenames, so dynamic imports / route chunks 404. Reload once to pick up
// the new HTML + asset references, with a cooldown so a genuinely broken
// build can't reload-loop the user.
function isChunkLoadError(reason: unknown): boolean {
	if (!reason) return false;
	const err = reason as { name?: string; message?: string };
	if (err.name === "ChunkLoadError") return true;
	const msg = err.message ?? String(reason);
	return (
		/Loading chunk [^\s]+ failed/i.test(msg) ||
		/Loading CSS chunk [^\s]+ failed/i.test(msg) ||
		/Failed to fetch dynamically imported module/i.test(msg) ||
		/Importing a module script failed/i.test(msg)
	);
}

function reloadOnce() {
	try {
		const last = Number(sessionStorage.getItem(RELOAD_KEY) ?? 0);
		if (Date.now() - last < RELOAD_COOLDOWN_MS) return;
		sessionStorage.setItem(RELOAD_KEY, String(Date.now()));
	} catch {}
	window.location.reload();
}

export function ChunkErrorReload() {
	useEffect(() => {
		const onError = (e: ErrorEvent) => {
			if (isChunkLoadError(e.error ?? e.message)) reloadOnce();
		};
		const onRejection = (e: PromiseRejectionEvent) => {
			if (isChunkLoadError(e.reason)) reloadOnce();
		};
		window.addEventListener("error", onError);
		window.addEventListener("unhandledrejection", onRejection);
		return () => {
			window.removeEventListener("error", onError);
			window.removeEventListener("unhandledrejection", onRejection);
		};
	}, []);
	return null;
}
