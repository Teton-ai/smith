"use client";

import {
	Activity,
	Cpu,
	FileText,
	Globe,
	Home,
	Layers,
	Smartphone,
	Terminal,
} from "lucide-react";
import { useRouter } from "next/navigation";
import type React from "react";
import { useEffect, useState } from "react";
import Profile from "@/app/components/profile";
import Sidebar from "@/app/components/sidebar";
import { useConfig } from "@/app/hooks/config";

const navigationItems = [
	{ path: "/dashboard", label: "Dashboard", icon: Home },
	{ path: "/devices", label: "Devices", icon: Cpu, shortcut: "S" },
	{
		path: "/distributions",
		label: "Distributions",
		icon: Layers,
		shortcut: "D",
	},
	{ path: "/commands", label: "Commands", icon: Terminal },
	{ path: "/ip-addresses", label: "IP Addresses", icon: Globe },
	{ path: "/modems", label: "Modems", icon: Smartphone },
	{ path: "/network-testing", label: "Network Analyzer", icon: Activity },
];

const bottomItems = [
	{
		path: "https://docs.smith.teton.ai",
		label: "Docs",
		icon: FileText,
		external: true,
	},
];

function useApiVersion() {
	const { config } = useConfig();
	const [version, setVersion] = useState<string | null>(null);

	useEffect(() => {
		if (!config?.API_BASE_URL) return;
		const controller = new AbortController();
		fetch(`${config.API_BASE_URL}/health`, { signal: controller.signal })
			.then((res) => {
				if (!res.ok) return;
				return res.text().then((text) => {
					// Response format: "I'm good: 0.2.116"
					const match = text.match(/:\s*(.+)/);
					if (match) setVersion(match[1].trim());
				});
			})
			.catch((err) => {
				if (err?.name === "AbortError") return;
				console.error("Failed to fetch API version:", err);
			});
		return () => controller.abort();
	}, [config?.API_BASE_URL]);

	return version;
}

const keyboardShortcuts: Record<string, string> = {
	s: "/devices",
	d: "/distributions",
};

function useKeyboardNav() {
	const router = useRouter();
	useEffect(() => {
		function handleKeyDown(e: KeyboardEvent) {
			if (e.ctrlKey || e.metaKey || e.altKey) return;
			const target = e.target as HTMLElement;
			if (
				target.tagName === "INPUT" ||
				target.tagName === "TEXTAREA" ||
				target.tagName === "SELECT" ||
				target.isContentEditable
			)
				return;
			const path = keyboardShortcuts[e.key];
			if (path) router.push(path);
		}
		document.addEventListener("keydown", handleKeyDown);
		return () => document.removeEventListener("keydown", handleKeyDown);
	}, [router]);
}

export default function PrivateLayout({
	children,
}: Readonly<{
	children: React.ReactNode;
}>) {
	const apiVersion = useApiVersion();
	useKeyboardNav();

	return (
		<div className="flex h-screen overflow-hidden bg-gray-50">
			<Sidebar
				items={navigationItems}
				bottomItems={bottomItems}
				bottomContent={(expanded) => <Profile sidebar expanded={expanded} />}
				mobileBottomContent={<Profile sidebar expanded />}
				versionText={apiVersion || undefined}
			/>
			<main className="flex-1 min-w-0 overflow-hidden mt-14 md:mt-0 flex flex-col">
				{children}
			</main>
		</div>
	);
}
