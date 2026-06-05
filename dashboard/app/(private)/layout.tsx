import { useAuth0 } from "@auth0/auth0-react";
import {
	Activity,
	Cpu,
	FileText,
	Globe,
	Home,
	Layers,
	ScrollText,
	Smartphone,
	Terminal,
} from "lucide-react";
import { useEffect, useState } from "react";
import { Outlet, useNavigate } from "react-router";
import { CommandPalette } from "@/app/components/CommandPalette";
import Profile from "@/app/components/profile";
import Sidebar, { type NavItem } from "@/app/components/sidebar";
import { useConfig } from "@/app/hooks/config";

const navigationItems: NavItem[] = [
	{ path: "/dashboard", label: "Dashboard", icon: Home },
	{ path: "/devices", label: "Devices", icon: Cpu, shortcut: "S" },
	{
		path: "/distributions",
		label: "Distributions",
		icon: Layers,
		shortcut: "D",
	},
	{ path: "/commands", label: "Commands", icon: Terminal },
	{ path: "/recipes", label: "Recipes", icon: ScrollText },
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
	const navigate = useNavigate();
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
			if (path) navigate(path);
		}
		document.addEventListener("keydown", handleKeyDown);
		return () => document.removeEventListener("keydown", handleKeyDown);
	}, [navigate]);
}

function useCommandPaletteShortcut(open: () => void) {
	useEffect(() => {
		function handleKeyDown(e: KeyboardEvent) {
			if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
				e.preventDefault();
				open();
			}
		}
		document.addEventListener("keydown", handleKeyDown);
		return () => document.removeEventListener("keydown", handleKeyDown);
	}, [open]);
}

export default function PrivateLayout() {
	const { isAuthenticated, isLoading } = useAuth0();
	const navigate = useNavigate();
	const apiVersion = useApiVersion();
	const [paletteOpen, setPaletteOpen] = useState(false);
	useKeyboardNav();
	useCommandPaletteShortcut(() => setPaletteOpen(true));

	useEffect(() => {
		if (!isLoading && !isAuthenticated) {
			navigate("/");
		}
	}, [isLoading, isAuthenticated, navigate]);

	if (isLoading || !isAuthenticated) {
		return null;
	}

	return (
		<div className="flex h-screen overflow-hidden bg-gray-50">
			<Sidebar
				items={navigationItems}
				bottomItems={bottomItems}
				bottomContent={(expanded) => <Profile sidebar expanded={expanded} />}
				mobileBottomContent={<Profile sidebar expanded />}
				versionText={apiVersion || undefined}
				onSearch={() => setPaletteOpen(true)}
			/>
			<main className="flex-1 min-w-0 overflow-hidden mt-14 md:mt-0 flex flex-col">
				<Outlet />
			</main>
			<CommandPalette
				open={paletteOpen}
				onClose={() => setPaletteOpen(false)}
			/>
		</div>
	);
}
