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
import type React from "react";
import { useEffect, useState } from "react";
import Profile from "@/app/components/profile";
import Sidebar from "@/app/components/sidebar";
import { useConfig } from "@/app/hooks/config";

const navigationItems = [
	{ path: "/dashboard", label: "Dashboard", icon: Home },
	{ path: "/devices", label: "Devices", icon: Cpu },
	{ path: "/distributions", label: "Distributions", icon: Layers },
	{ path: "/commands", label: "Commands", icon: Terminal },
	{ path: "/ip-addresses", label: "IP Addresses", icon: Globe },
	{ path: "/modems", label: "Modems", icon: Smartphone },
	{ path: "/hackathon", label: "Network Analyzer", icon: Activity },
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

export default function PrivateLayout({
	children,
}: Readonly<{
	children: React.ReactNode;
}>) {
	const apiVersion = useApiVersion();

	return (
		<Sidebar
			items={navigationItems}
			bottomItems={bottomItems}
			bottomContent={<Profile sidebar />}
			mobileBottomContent={<Profile sidebar expanded />}
			versionText={apiVersion || undefined}
		>
			<div className="px-4 sm:px-6 lg:px-8 py-6">{children}</div>
		</Sidebar>
	);
}
