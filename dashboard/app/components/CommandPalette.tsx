"use client";

import {
	Activity,
	BarChart3,
	ChevronRight,
	Cpu,
	FileText,
	Globe,
	Home,
	Layers,
	Power,
	Search,
	Smartphone,
	Terminal,
} from "lucide-react";
import {
	type ReactNode,
	useCallback,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";
import { useNavigate } from "react-router";
import {
	type DeviceService,
	useDeviceServices,
} from "@/app/(private)/devices/[serial]/services/useDeviceServices";
import {
	type Device,
	type Distribution,
	useGetDevices,
	useGetDistributions,
	useIssueCommandsToDevices,
} from "@/app/api-client";
import { Button } from "@/app/components/button";
import { useConfig } from "@/app/hooks/config";

type ActionId = "reboot" | "run" | "grafana" | "services" | "logs";

interface Action {
	id: ActionId;
	label: string;
	hint: string;
	icon: ReactNode;
	keywords: string[];
}

const ACTIONS: Action[] = [
	{
		id: "reboot",
		label: "Reboot device",
		hint: "Restart a device",
		icon: <Power className="w-4 h-4" />,
		keywords: ["reboot", "restart", "reset"],
	},
	{
		id: "run",
		label: "Run command",
		hint: "Execute a shell command on a device",
		icon: <Terminal className="w-4 h-4" />,
		keywords: ["run", "command", "exec", "shell"],
	},
	{
		id: "services",
		label: "Open services",
		hint: "View services running on a device",
		icon: <FileText className="w-4 h-4" />,
		keywords: ["services", "service"],
	},
	{
		id: "logs",
		label: "Stream service logs",
		hint: "Pick a device + service and stream its logs",
		icon: <FileText className="w-4 h-4" />,
		keywords: ["logs", "log", "tail", "stream"],
	},
	{
		id: "grafana",
		label: "Open Grafana",
		hint: "Open a device's Grafana dashboard",
		icon: <BarChart3 className="w-4 h-4" />,
		keywords: ["grafana", "metrics", "dashboard"],
	},
];

interface Page {
	label: string;
	path: string;
	icon: ReactNode;
	keywords: string[];
	external?: boolean;
}

const PAGES: Page[] = [
	{
		label: "Dashboard",
		path: "/dashboard",
		icon: <Home className="w-4 h-4" />,
		keywords: ["dashboard", "home"],
	},
	{
		label: "Devices",
		path: "/devices",
		icon: <Cpu className="w-4 h-4" />,
		keywords: ["devices", "fleet"],
	},
	{
		label: "Distributions",
		path: "/distributions",
		icon: <Layers className="w-4 h-4" />,
		keywords: ["distributions", "releases", "dist"],
	},
	{
		label: "Commands",
		path: "/commands",
		icon: <Terminal className="w-4 h-4" />,
		keywords: ["commands", "queue", "history"],
	},
	{
		label: "IP Addresses",
		path: "/ip-addresses",
		icon: <Globe className="w-4 h-4" />,
		keywords: ["ip", "addresses", "network"],
	},
	{
		label: "Modems",
		path: "/modems",
		icon: <Smartphone className="w-4 h-4" />,
		keywords: ["modems", "cellular", "sim"],
	},
	{
		label: "Network Analyzer",
		path: "/network-testing",
		icon: <Activity className="w-4 h-4" />,
		keywords: ["network", "analyzer", "testing"],
	},
	{
		label: "Docs",
		path: "https://docs.smith.teton.ai",
		icon: <FileText className="w-4 h-4" />,
		keywords: ["docs", "documentation", "help"],
		external: true,
	},
];

type Kind = "command" | "page" | "device" | "distribution";

type RootItem =
	| { kind: "command"; action: Action }
	| { kind: "page"; page: Page }
	| { kind: "device"; device: Device }
	| { kind: "distribution"; distribution: Distribution };

type Step =
	| { kind: "root" }
	| { kind: "device"; action: Action }
	| { kind: "service"; action: Action; device: Device }
	| { kind: "runInput"; device: Device }
	| { kind: "confirmRun"; device: Device; command: string }
	| { kind: "confirmReboot"; device: Device };

interface CommandPaletteProps {
	open: boolean;
	onClose: () => void;
}

function matches(query: string, ...fields: string[]): boolean {
	if (!query) return true;
	const q = query.toLowerCase();
	return fields.some((f) => f.toLowerCase().includes(q));
}

function useDebounced<T>(value: T, ms: number): T {
	const [debounced, setDebounced] = useState(value);
	useEffect(() => {
		const t = setTimeout(() => setDebounced(value), ms);
		return () => clearTimeout(t);
	}, [value, ms]);
	return debounced;
}

const KIND_LABELS: Record<Kind, string> = {
	command: "Command",
	page: "Page",
	device: "Device",
	distribution: "Distribution",
};

function Highlight({ text, query }: { text: string; query: string }) {
	if (!query) return <>{text}</>;
	const q = query.toLowerCase();
	const lower = text.toLowerCase();
	const parts: ReactNode[] = [];
	let i = 0;
	let keySeed = 0;
	while (i <= text.length) {
		const idx = lower.indexOf(q, i);
		if (idx === -1) {
			if (i < text.length) parts.push(text.slice(i));
			break;
		}
		if (idx > i) parts.push(text.slice(i, idx));
		parts.push(
			<span key={`h-${keySeed++}`} className="font-semibold text-gray-900">
				{text.slice(idx, idx + q.length)}
			</span>,
		);
		i = idx + q.length;
	}
	return <>{parts}</>;
}

export function CommandPalette({ open, onClose }: CommandPaletteProps) {
	const navigate = useNavigate();
	const { config } = useConfig();
	const inputRef = useRef<HTMLInputElement>(null);
	const listRef = useRef<HTMLDivElement>(null);

	const [step, setStep] = useState<Step>({ kind: "root" });
	const [query, setQuery] = useState("");
	const [activeIndex, setActiveIndex] = useState(0);

	const trimmed = query.trim();
	const debouncedSearch = useDebounced(trimmed, 150);

	const showDeviceResults =
		step.kind === "root" ? debouncedSearch.length > 0 : step.kind === "device";

	const { data: devices = [] } = useGetDevices(
		{
			search: debouncedSearch || undefined,
			limit: step.kind === "device" ? 50 : 8,
		},
		{ query: { enabled: open && showDeviceResults } },
	);

	const { data: distributions = [] } = useGetDistributions(undefined, {
		query: { enabled: open && step.kind === "root" },
	});

	const serviceDeviceSerial =
		step.kind === "service" ? step.device.serial_number : "";
	const { data: services = [] } = useDeviceServices(serviceDeviceSerial);

	const pendingNavRef = useRef<string | null>(null);

	const close = useCallback(() => {
		onClose();
	}, [onClose]);

	const { mutate: issueCommands, isPending: issuing } =
		useIssueCommandsToDevices({
			mutation: {
				onSuccess: () => {
					const target = pendingNavRef.current;
					pendingNavRef.current = null;
					close();
					if (target) navigate(target);
				},
				onError: (e) => {
					pendingNavRef.current = null;
					console.error("Command failed:", e);
				},
			},
		});

	useEffect(() => {
		if (open) {
			setStep({ kind: "root" });
			setQuery("");
			setActiveIndex(0);
			requestAnimationFrame(() => inputRef.current?.focus());
		}
	}, [open]);

	// biome-ignore lint/correctness/useExhaustiveDependencies: reset selection when step changes
	useEffect(() => {
		setActiveIndex(0);
	}, [query, step.kind]);

	const rootItems = useMemo<RootItem[]>(() => {
		if (step.kind !== "root") return [];
		const out: RootItem[] = [];

		for (const a of ACTIONS) {
			if (
				matches(trimmed, a.label, a.hint, ...a.keywords) ||
				a.keywords.some((k) => k.startsWith(trimmed.toLowerCase()))
			) {
				out.push({ kind: "command", action: a });
			}
		}
		for (const p of PAGES) {
			if (matches(trimmed, p.label, ...p.keywords)) {
				out.push({ kind: "page", page: p });
			}
		}
		if (trimmed) {
			for (const d of devices.slice(0, 8)) {
				out.push({ kind: "device", device: d });
			}
			for (const dist of distributions) {
				if (matches(trimmed, dist.name, String(dist.id))) {
					out.push({ kind: "distribution", distribution: dist });
				}
			}
		}
		return out;
	}, [step.kind, trimmed, devices, distributions]);

	const stepItems = useMemo(() => {
		if (step.kind === "device") return devices;
		if (step.kind === "service") {
			return services
				.filter((s) => matches(trimmed, s.service_name))
				.slice(0, 50);
		}
		return [] as unknown[];
	}, [step.kind, devices, services, trimmed]);

	const runGrafana = useCallback(
		(device: Device) => {
			const tpl = config?.DEVICE_GRAFANA_URL;
			if (!tpl) {
				console.error("DEVICE_GRAFANA_URL not configured");
				return;
			}
			const url = tpl.replace("{serial_number}", device.serial_number);
			window.open(url, "_blank", "noopener,noreferrer");
			close();
		},
		[config?.DEVICE_GRAFANA_URL, close],
	);

	const runActionForDevice = useCallback(
		(action: Action, device: Device) => {
			if (action.id === "services") {
				navigate(`/devices/${device.serial_number}/services`);
				close();
				return;
			}
			if (action.id === "logs") {
				setStep({ kind: "service", action, device });
				setQuery("");
				return;
			}
			if (action.id === "grafana") {
				runGrafana(device);
				return;
			}
			if (action.id === "run") {
				setStep({ kind: "runInput", device });
				setQuery("");
				return;
			}
			if (action.id === "reboot") {
				setStep({ kind: "confirmReboot", device });
				return;
			}
		},
		[navigate, close, runGrafana],
	);

	const onPickRoot = useCallback(
		(item: RootItem) => {
			if (item.kind === "command") {
				setStep({ kind: "device", action: item.action });
				setQuery("");
				return;
			}
			if (item.kind === "page") {
				if (item.page.external) {
					window.open(item.page.path, "_blank", "noopener,noreferrer");
				} else {
					navigate(item.page.path);
				}
				close();
				return;
			}
			if (item.kind === "device") {
				navigate(`/devices/${item.device.serial_number}`);
				close();
				return;
			}
			if (item.kind === "distribution") {
				navigate(`/distributions/${item.distribution.id}`);
				close();
				return;
			}
		},
		[navigate, close],
	);

	const onPickService = useCallback(
		(service: DeviceService) => {
			if (step.kind !== "service") return;
			navigate(
				`/devices/${step.device.serial_number}/services?service=${encodeURIComponent(service.service_name)}`,
			);
			close();
		},
		[step, navigate, close],
	);

	const goToRunConfirm = useCallback(() => {
		if (step.kind !== "runInput") return;
		const cmd = query.trim();
		if (!cmd) return;
		setStep({ kind: "confirmRun", device: step.device, command: cmd });
		setQuery("");
	}, [step, query]);

	const submitRun = useCallback(() => {
		if (step.kind !== "confirmRun") return;
		pendingNavRef.current = `/devices/${step.device.serial_number}/commands`;
		issueCommands({
			data: {
				devices: [step.device.id],
				commands: [
					{
						id: -1,
						command: { FreeForm: { cmd: step.command } },
						continue_on_error: false,
					},
				],
			},
		});
	}, [step, issueCommands]);

	const submitReboot = useCallback(() => {
		if (step.kind !== "confirmReboot") return;
		pendingNavRef.current = `/devices/${step.device.serial_number}/commands`;
		issueCommands({
			data: {
				devices: [step.device.id],
				commands: [{ id: -1, command: "Restart", continue_on_error: false }],
			},
		});
	}, [step, issueCommands]);

	const goBack = useCallback(() => {
		if (step.kind === "root") {
			close();
		} else if (step.kind === "service") {
			setStep({ kind: "device", action: step.action });
			setQuery("");
		} else if (step.kind === "confirmRun") {
			setStep({ kind: "runInput", device: step.device });
			setQuery(step.command);
		} else if (step.kind === "runInput" || step.kind === "confirmReboot") {
			const actionId = step.kind === "runInput" ? "run" : "reboot";
			const action = ACTIONS.find((a) => a.id === actionId);
			if (action) {
				setStep({ kind: "device", action });
				setQuery("");
			}
		} else {
			setStep({ kind: "root" });
			setQuery("");
		}
	}, [step, close]);

	const listLength =
		step.kind === "root"
			? rootItems.length
			: step.kind === "device" || step.kind === "service"
				? stepItems.length
				: 0;

	useEffect(() => {
		if (!open) return;
		const onKey = (e: KeyboardEvent) => {
			if (e.key === "Escape") {
				e.preventDefault();
				close();
				return;
			}
			if (step.kind === "confirmReboot") {
				if (e.key === "Enter") {
					e.preventDefault();
					submitReboot();
				} else if (e.key === "Backspace" && query === "") {
					e.preventDefault();
					goBack();
				}
				return;
			}
			if (step.kind === "confirmRun") {
				if (e.key === "Enter") {
					e.preventDefault();
					submitRun();
				} else if (e.key === "Backspace" && query === "") {
					e.preventDefault();
					goBack();
				}
				return;
			}
			if (step.kind === "runInput") {
				if (e.key === "Enter" && !e.shiftKey) {
					e.preventDefault();
					goToRunConfirm();
				} else if (e.key === "Backspace" && query === "") {
					e.preventDefault();
					goBack();
				}
				return;
			}
			if (e.key === "ArrowDown") {
				e.preventDefault();
				setActiveIndex((i) => Math.min(i + 1, Math.max(listLength - 1, 0)));
			} else if (e.key === "ArrowUp") {
				e.preventDefault();
				setActiveIndex((i) => Math.max(i - 1, 0));
			} else if (e.key === "Enter") {
				e.preventDefault();
				if (step.kind === "root") {
					const item = rootItems[activeIndex];
					if (item) onPickRoot(item);
				} else if (step.kind === "device") {
					const d = (stepItems as Device[])[activeIndex];
					if (d) runActionForDevice(step.action, d);
				} else if (step.kind === "service") {
					const s = (stepItems as DeviceService[])[activeIndex];
					if (s) onPickService(s);
				}
			} else if (
				e.key === "Backspace" &&
				query === "" &&
				step.kind !== "root"
			) {
				e.preventDefault();
				goBack();
			}
		};
		document.addEventListener("keydown", onKey);
		return () => document.removeEventListener("keydown", onKey);
	}, [
		open,
		step,
		rootItems,
		stepItems,
		listLength,
		activeIndex,
		query,
		onPickRoot,
		runActionForDevice,
		onPickService,
		goToRunConfirm,
		submitRun,
		submitReboot,
		goBack,
		close,
	]);

	useEffect(() => {
		const el = listRef.current?.querySelector<HTMLElement>(
			`[data-idx="${activeIndex}"]`,
		);
		el?.scrollIntoView({ block: "nearest" });
	}, [activeIndex]);

	if (!open) return null;

	const placeholder = (() => {
		switch (step.kind) {
			case "root":
				return "Search devices, distributions, commands, pages...";
			case "device":
				return `Pick device for "${step.action.label}"...`;
			case "service":
				return `Pick service on ${step.device.serial_number}...`;
			case "runInput":
				return `Shell command to run on ${step.device.serial_number}`;
			case "confirmRun":
				return `Press Enter to run on ${step.device.serial_number}`;
			case "confirmReboot":
				return `Press Enter to reboot ${step.device.serial_number}`;
		}
	})();

	const breadcrumbs: string[] = [];
	if (step.kind === "device") breadcrumbs.push(step.action.label);
	else if (step.kind === "service")
		breadcrumbs.push(step.action.label, step.device.serial_number);
	else if (step.kind === "runInput")
		breadcrumbs.push("Run command", step.device.serial_number);
	else if (step.kind === "confirmRun")
		breadcrumbs.push("Run command", step.device.serial_number, "Confirm");
	else if (step.kind === "confirmReboot")
		breadcrumbs.push("Reboot", step.device.serial_number);

	return createPortal(
		<div
			onClick={(e) => {
				if (e.target === e.currentTarget) close();
			}}
			className="fixed inset-0 bg-black/40 z-50 flex items-start justify-center pt-[10vh] px-4"
		>
			<div className="bg-white rounded-lg shadow-2xl w-full max-w-2xl overflow-hidden flex flex-col">
				{breadcrumbs.length > 0 && (
					<div className="flex items-center gap-1 px-4 pt-3 text-xs text-gray-500 flex-wrap">
						<button
							type="button"
							onClick={goBack}
							className="hover:text-gray-700 cursor-pointer"
						>
							Search
						</button>
						{breadcrumbs.map((b, i) => (
							<span key={`${b}-${i}`} className="flex items-center gap-1">
								<ChevronRight className="w-3 h-3" />
								<span className="text-gray-700 font-medium">{b}</span>
							</span>
						))}
					</div>
				)}

				<div className="flex items-center gap-2 px-4 py-3 border-b border-gray-100">
					<Search className="w-4 h-4 text-gray-400 flex-shrink-0" />
					<input
						ref={inputRef}
						type="text"
						value={query}
						onChange={(e) => setQuery(e.target.value)}
						placeholder={placeholder}
						className="flex-1 outline-none text-sm text-gray-900 placeholder-gray-400 bg-transparent"
					/>
					<kbd className="text-[10px] text-gray-400 border border-gray-200 rounded px-1.5 py-0.5">
						Esc
					</kbd>
				</div>

				{step.kind === "confirmReboot" ? (
					<div className="p-6 text-sm">
						<p className="text-gray-900">
							Reboot{" "}
							<span className="font-mono font-semibold">
								{step.device.serial_number}
							</span>
							?
						</p>
						<p className="text-gray-500 mt-1 text-xs">
							The device will be temporarily offline. Press Enter to confirm.
						</p>
						<div className="flex justify-end gap-2 mt-4">
							<Button variant="secondary" disabled={issuing} onClick={goBack}>
								Back
							</Button>
							<Button variant="danger" loading={issuing} onClick={submitReboot}>
								{issuing ? "Rebooting..." : "Reboot"}
							</Button>
						</div>
					</div>
				) : step.kind === "confirmRun" ? (
					<div className="p-6 text-sm">
						<p className="text-gray-900">
							Run command on{" "}
							<span className="font-mono font-semibold">
								{step.device.serial_number}
							</span>
							?
						</p>
						<pre className="mt-3 bg-gray-900 text-white text-xs font-mono p-3 rounded overflow-x-auto whitespace-pre-wrap break-all">
							{step.command}
						</pre>
						<p className="text-gray-500 mt-2 text-xs">
							Queued and executed when the device next checks in. Press Enter to
							confirm.
						</p>
						<div className="flex justify-end gap-2 mt-4">
							<Button variant="secondary" disabled={issuing} onClick={goBack}>
								Back
							</Button>
							<Button variant="purple" loading={issuing} onClick={submitRun}>
								{issuing ? "Sending..." : "Run command"}
							</Button>
						</div>
					</div>
				) : step.kind === "runInput" ? (
					<div className="p-4 text-xs text-gray-500">
						Press{" "}
						<kbd className="border border-gray-200 rounded px-1">Enter</kbd> to
						review,{" "}
						<kbd className="border border-gray-200 rounded px-1">Backspace</kbd>{" "}
						(empty) to go back.
					</div>
				) : (
					<div ref={listRef} className="max-h-96 overflow-y-auto py-1">
						{listLength === 0 ? (
							<div className="px-4 py-6 text-sm text-gray-400 text-center">
								No matches
							</div>
						) : step.kind === "root" ? (
							rootItems.map((item, i) => (
								<RootRow
									key={`${item.kind}-${
										item.kind === "command"
											? item.action.id
											: item.kind === "page"
												? item.page.path
												: item.kind === "device"
													? item.device.id
													: item.distribution.id
									}`}
									idx={i}
									active={i === activeIndex}
									item={item}
									query={trimmed}
									onClick={() => {
										setActiveIndex(i);
										onPickRoot(item);
									}}
								/>
							))
						) : step.kind === "device" ? (
							(stepItems as Device[]).map((d, i) => (
								<Row
									key={d.id}
									idx={i}
									active={i === activeIndex}
									onClick={() => {
										setActiveIndex(i);
										runActionForDevice(step.action, d);
									}}
									icon={
										<span
											className={`w-2 h-2 rounded-full ${
												d.online ? "bg-green-500" : "bg-gray-300"
											}`}
										/>
									}
									title={d.serial_number}
									subtitle={
										Object.entries(d.labels || {})
											.slice(0, 3)
											.map(([k, v]) => `${k}=${v}`)
											.join("  ") || `id ${d.id}`
									}
									kind="device"
									query={trimmed}
								/>
							))
						) : (
							(stepItems as DeviceService[]).map((s, i) => (
								<Row
									key={s.id}
									idx={i}
									active={i === activeIndex}
									onClick={() => {
										setActiveIndex(i);
										onPickService(s);
									}}
									icon={
										<span
											className={`w-2 h-2 rounded-full ${
												s.active_state === "active"
													? "bg-green-500"
													: s.active_state
														? "bg-red-500"
														: "bg-gray-300"
											}`}
										/>
									}
									title={s.service_name}
									subtitle={s.active_state || "unknown"}
									query={trimmed}
								/>
							))
						)}
					</div>
				)}

				<div className="flex items-center justify-between px-4 py-2 border-t border-gray-100 text-[11px] text-gray-400">
					<div className="flex items-center gap-3">
						<span>
							<kbd className="border border-gray-200 rounded px-1">↑↓</kbd>{" "}
							navigate
						</span>
						<span>
							<kbd className="border border-gray-200 rounded px-1">Enter</kbd>{" "}
							select
						</span>
						<span>
							<kbd className="border border-gray-200 rounded px-1">⌫</kbd> back
						</span>
					</div>
					<span>⌘K</span>
				</div>
			</div>
		</div>,
		document.body,
	);
}

interface RowProps {
	idx: number;
	active: boolean;
	onClick: () => void;
	icon: ReactNode;
	title: string;
	subtitle?: string;
	kind?: Kind;
	query?: string;
}

function Row({
	idx,
	active,
	onClick,
	icon,
	title,
	subtitle,
	kind,
	query = "",
}: RowProps) {
	return (
		<button
			type="button"
			data-idx={idx}
			onClick={onClick}
			onMouseDown={(e) => e.preventDefault()}
			className={`w-full text-left flex items-center gap-3 px-4 py-2.5 cursor-pointer ${
				active ? "bg-blue-50" : "hover:bg-gray-50"
			}`}
		>
			<span
				className={`flex-shrink-0 ${
					active ? "text-blue-600" : "text-gray-500"
				}`}
			>
				{icon}
			</span>
			<span className="min-w-0 flex-1">
				<span className="block text-sm text-gray-900 truncate">
					<Highlight text={title} query={query} />
				</span>
				{subtitle && (
					<span className="block text-xs text-gray-500 truncate">
						<Highlight text={subtitle} query={query} />
					</span>
				)}
			</span>
			{kind && (
				<span className="flex-shrink-0 text-[10px] uppercase tracking-wide text-gray-400">
					{KIND_LABELS[kind]}
				</span>
			)}
		</button>
	);
}

function RootRow({
	idx,
	active,
	item,
	onClick,
	query,
}: {
	idx: number;
	active: boolean;
	item: RootItem;
	onClick: () => void;
	query: string;
}) {
	if (item.kind === "command") {
		return (
			<Row
				idx={idx}
				active={active}
				onClick={onClick}
				icon={item.action.icon}
				title={item.action.label}
				subtitle={item.action.hint}
				kind="command"
				query={query}
			/>
		);
	}
	if (item.kind === "page") {
		return (
			<Row
				idx={idx}
				active={active}
				onClick={onClick}
				icon={item.page.icon}
				title={item.page.label}
				subtitle={item.page.external ? "External link" : item.page.path}
				kind="page"
				query={query}
			/>
		);
	}
	if (item.kind === "device") {
		const d = item.device;
		return (
			<Row
				idx={idx}
				active={active}
				onClick={onClick}
				icon={
					<span
						className={`w-2 h-2 rounded-full ${
							d.online ? "bg-green-500" : "bg-gray-300"
						}`}
					/>
				}
				title={d.serial_number}
				subtitle={
					Object.entries(d.labels || {})
						.slice(0, 3)
						.map(([k, v]) => `${k}=${v}`)
						.join("  ") || `id ${d.id}`
				}
				kind="device"
				query={query}
			/>
		);
	}
	const dist = item.distribution;
	return (
		<Row
			idx={idx}
			active={active}
			onClick={onClick}
			icon={<Layers className="w-4 h-4" />}
			title={dist.name}
			subtitle={`${dist.architecture}${dist.num_packages != null ? `  ·  ${dist.num_packages} packages` : ""}`}
			kind="distribution"
			query={query}
		/>
	);
}
