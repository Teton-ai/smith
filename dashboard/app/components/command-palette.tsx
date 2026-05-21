import {
	Activity,
	CornerDownLeft,
	Cpu,
	FileText,
	Globe,
	Home,
	Layers,
	Loader2,
	Search,
	Smartphone,
	Terminal,
} from "lucide-react";
import {
	type ComponentType,
	useEffect,
	useId,
	useMemo,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";
import { useNavigate } from "react-router";
import {
	type Device,
	type Distribution,
	useGetDevices,
	useGetDistributions,
} from "@/app/api-client";

type NavItem = {
	label: string;
	path: string;
	icon: ComponentType<{ className?: string }>;
	keywords?: string;
	external?: boolean;
};

const NAV_ITEMS: NavItem[] = [
	{ label: "Dashboard", path: "/dashboard", icon: Home, keywords: "home" },
	{ label: "Devices", path: "/devices", icon: Cpu },
	{ label: "Distributions", path: "/distributions", icon: Layers },
	{ label: "Commands", path: "/commands", icon: Terminal },
	{ label: "IP Addresses", path: "/ip-addresses", icon: Globe, keywords: "ip" },
	{ label: "Modems", path: "/modems", icon: Smartphone },
	{
		label: "Network Analyzer",
		path: "/network-testing",
		icon: Activity,
		keywords: "network testing",
	},
	{
		label: "Docs",
		path: "https://docs.smith.teton.ai",
		icon: FileText,
		keywords: "documentation help",
		external: true,
	},
];

function useDebounced<T>(value: T, delay: number): T {
	const [debounced, setDebounced] = useState(value);
	useEffect(() => {
		const t = setTimeout(() => setDebounced(value), delay);
		return () => clearTimeout(t);
	}, [value, delay]);
	return debounced;
}

interface CommandPaletteProps {
	open: boolean;
	onClose: () => void;
}

type Row =
	| { kind: "nav"; item: NavItem }
	| { kind: "distribution"; distribution: Distribution }
	| { kind: "device"; device: Device };

export function CommandPalette({ open, onClose }: CommandPaletteProps) {
	const navigate = useNavigate();
	const [query, setQuery] = useState("");
	const [selected, setSelected] = useState(0);
	const [mounted, setMounted] = useState(false);
	const [visible, setVisible] = useState(false);
	const inputRef = useRef<HTMLInputElement>(null);
	const listRef = useRef<HTMLDivElement>(null);
	const backdropRef = useRef<HTMLDivElement>(null);
	const listboxId = useId();

	useEffect(() => {
		setMounted(true);
	}, []);

	useEffect(() => {
		if (open) {
			requestAnimationFrame(() => setVisible(true));
			setQuery("");
			setSelected(0);
		} else {
			setVisible(false);
		}
	}, [open]);

	useEffect(() => {
		if (open) {
			inputRef.current?.focus();
		}
	}, [open]);

	const debouncedQuery = useDebounced(query, 120);
	const trimmed = debouncedQuery.trim();

	const devicesQuery = useGetDevices(
		{ search: trimmed, limit: 10 },
		{
			query: {
				enabled: open && trimmed.length > 0,
				staleTime: 10_000,
			},
		},
	);

	const distributionsQuery = useGetDistributions({
		query: {
			enabled: open,
			staleTime: 60_000,
		},
	});

	const filteredNav = useMemo(() => {
		const q = query.trim().toLowerCase();
		if (!q) return NAV_ITEMS;
		return NAV_ITEMS.filter((item) => {
			const haystack = `${item.label} ${item.keywords ?? ""}`.toLowerCase();
			return haystack.includes(q);
		});
	}, [query]);

	const filteredDistributions = useMemo(() => {
		const all = distributionsQuery.data ?? [];
		const q = query.trim().toLowerCase();
		if (!q) return all.slice(0, 8);
		return all.filter((d) => {
			const haystack =
				`${d.name} ${d.architecture} ${d.description ?? ""}`.toLowerCase();
			return haystack.includes(q);
		});
	}, [distributionsQuery.data, query]);

	const rows: Row[] = useMemo(() => {
		const navRows: Row[] = filteredNav.map((item) => ({ kind: "nav", item }));
		const distRows: Row[] = filteredDistributions.map((distribution) => ({
			kind: "distribution",
			distribution,
		}));
		const deviceRows: Row[] = (devicesQuery.data ?? []).map((device) => ({
			kind: "device",
			device,
		}));
		return [...navRows, ...distRows, ...deviceRows];
	}, [filteredNav, filteredDistributions, devicesQuery.data]);

	useEffect(() => {
		if (selected >= rows.length) setSelected(Math.max(0, rows.length - 1));
	}, [rows.length, selected]);

	useEffect(() => {
		const el = listRef.current?.querySelector<HTMLElement>(
			`[data-index="${selected}"]`,
		);
		el?.scrollIntoView({ block: "nearest" });
	}, [selected]);

	function activate(row: Row) {
		if (row.kind === "nav") {
			if (row.item.external) {
				window.open(row.item.path, "_blank", "noopener,noreferrer");
			} else {
				navigate(row.item.path);
			}
		} else if (row.kind === "distribution") {
			navigate(`/distributions/${row.distribution.id}`);
		} else {
			navigate(`/devices/${row.device.serial_number}`);
		}
		onClose();
	}

	function onKeyDown(e: React.KeyboardEvent) {
		if (e.key === "ArrowDown") {
			e.preventDefault();
			setSelected((s) => Math.min(s + 1, rows.length - 1));
		} else if (e.key === "ArrowUp") {
			e.preventDefault();
			setSelected((s) => Math.max(s - 1, 0));
		} else if (e.key === "Enter") {
			e.preventDefault();
			const row = rows[selected];
			if (row) activate(row);
		} else if (e.key === "Escape") {
			e.preventDefault();
			onClose();
		}
	}

	if (!mounted || !open) return null;

	const navCount = filteredNav.length;
	const distCount = filteredDistributions.length;
	const showDevicesSection = trimmed.length > 0;
	const noResults =
		rows.length === 0 && !showDevicesSection && !distributionsQuery.isLoading;

	return createPortal(
		<div
			ref={backdropRef}
			onClick={(e) => {
				if (e.target === backdropRef.current) onClose();
			}}
			className={`fixed inset-0 bg-gray-900/40 backdrop-blur-[2px] flex items-start justify-center z-50 p-4 pt-[15vh] transition-opacity duration-150 ${
				visible ? "opacity-100" : "opacity-0"
			}`}
		>
			<div
				className={`bg-white rounded-lg shadow-xl ring-1 ring-black/5 w-[640px] max-w-full max-h-[70vh] flex flex-col transition-all duration-150 ease-out ${
					visible ? "opacity-100 scale-100" : "opacity-0 scale-[0.98]"
				}`}
			>
				{/* Header / input */}
				<div className="flex items-center gap-3 px-4 h-12 border-b border-gray-200">
					<Search className="w-[18px] h-[18px] text-gray-400 shrink-0" />
					<input
						ref={inputRef}
						value={query}
						onChange={(e) => setQuery(e.target.value)}
						onKeyDown={onKeyDown}
						placeholder="Search devices, distributions, or jump to a section…"
						className="flex-1 bg-transparent outline-none text-sm text-gray-900 placeholder:text-gray-400"
						role="combobox"
						aria-expanded="true"
						aria-controls={listboxId}
						aria-activedescendant={
							rows[selected] ? `${listboxId}-opt-${selected}` : undefined
						}
					/>
					{showDevicesSection && devicesQuery.isFetching && (
						<Loader2 className="w-4 h-4 text-gray-400 animate-spin" />
					)}
				</div>

				{/* Results */}
				<div
					ref={listRef}
					id={listboxId}
					role="listbox"
					className="overflow-y-auto py-2 flex-1"
				>
					{navCount > 0 && <SectionHeader>Go to</SectionHeader>}
					{filteredNav.map((item, i) => {
						const Icon = item.icon;
						const isSelected = i === selected;
						return (
							<Option
								key={item.path}
								id={`${listboxId}-opt-${i}`}
								index={i}
								selected={isSelected}
								onClick={() => activate({ kind: "nav", item })}
								onMouseMove={() => setSelected(i)}
							>
								<Icon
									className={`w-[18px] h-[18px] shrink-0 ${
										isSelected ? "text-indigo-700" : "text-gray-500"
									}`}
								/>
								<span
									className={`text-sm ${
										isSelected ? "text-indigo-700 font-medium" : "text-gray-900"
									}`}
								>
									{item.label}
								</span>
								<span className="ml-auto text-[11px] text-gray-400 font-mono">
									{item.external ? "↗" : item.path}
								</span>
							</Option>
						);
					})}

					{distCount > 0 && <SectionHeader>Distributions</SectionHeader>}
					{filteredDistributions.map((distribution, i) => {
						const rowIndex = navCount + i;
						const isSelected = rowIndex === selected;
						return (
							<Option
								key={distribution.id}
								id={`${listboxId}-opt-${rowIndex}`}
								index={rowIndex}
								selected={isSelected}
								onClick={() => activate({ kind: "distribution", distribution })}
								onMouseMove={() => setSelected(rowIndex)}
							>
								<Layers
									className={`w-[18px] h-[18px] shrink-0 ${
										isSelected ? "text-indigo-700" : "text-gray-500"
									}`}
								/>
								<div className="flex flex-col min-w-0">
									<span
										className={`text-sm truncate ${
											isSelected
												? "text-indigo-700 font-medium"
												: "text-gray-900"
										}`}
									>
										{distribution.name}
									</span>
									<span className="text-xs text-gray-500 truncate">
										{distribution.architecture}
										{distribution.description
											? ` · ${distribution.description}`
											: ""}
									</span>
								</div>
								<span className="ml-auto text-[10px] uppercase tracking-wider text-gray-400 font-mono">
									Distribution
								</span>
							</Option>
						);
					})}

					{showDevicesSection && (
						<>
							<SectionHeader>Devices</SectionHeader>
							{devicesQuery.isLoading && (
								<div className="px-4 py-3 text-sm text-gray-400 flex items-center gap-2">
									<Loader2 className="w-4 h-4 animate-spin" />
									Searching…
								</div>
							)}
							{!devicesQuery.isLoading &&
								(devicesQuery.data ?? []).length === 0 && (
									<div className="px-4 py-3 text-sm text-gray-400">
										No devices match “{trimmed}”.
									</div>
								)}
							{(devicesQuery.data ?? []).map((device, i) => {
								const rowIndex = navCount + distCount + i;
								const isSelected = rowIndex === selected;
								const hostname = (
									device.system_info as { hostname?: string } | null
								)?.hostname;
								return (
									<Option
										key={device.id}
										id={`${listboxId}-opt-${rowIndex}`}
										index={rowIndex}
										selected={isSelected}
										onClick={() => activate({ kind: "device", device })}
										onMouseMove={() => setSelected(rowIndex)}
									>
										<Cpu
											className={`w-[18px] h-[18px] shrink-0 ${
												isSelected ? "text-indigo-700" : "text-gray-500"
											}`}
										/>
										<div className="flex flex-col min-w-0">
											<span
												className={`text-sm truncate font-mono ${
													isSelected
														? "text-indigo-700 font-medium"
														: "text-gray-900"
												}`}
											>
												{device.serial_number}
											</span>
											{hostname && (
												<span className="text-xs text-gray-500 truncate">
													{hostname}
												</span>
											)}
										</div>
										<span className="ml-auto inline-flex items-center gap-1.5 text-[11px] text-gray-500">
											<span
												className={`inline-block w-1.5 h-1.5 rounded-full ${
													device.online ? "bg-green-500" : "bg-gray-300"
												}`}
											/>
											{device.online ? "Online" : "Offline"}
										</span>
									</Option>
								);
							})}
						</>
					)}

					{noResults && (
						<div className="px-4 py-10 text-sm text-gray-400 text-center">
							No matches for “{query}”.
						</div>
					)}
				</div>

				{/* Footer */}
				<div className="border-t border-gray-200 px-4 h-9 flex items-center gap-4 text-[11px] text-gray-500">
					<Hint k={<>↑↓</>}>Navigate</Hint>
					<Hint k={<CornerDownLeft className="w-3 h-3" />}>Open</Hint>
					<Hint k="Esc">Close</Hint>
				</div>
			</div>
		</div>,
		document.body,
	);
}

function SectionHeader({ children }: { children: React.ReactNode }) {
	return (
		<div className="px-4 pt-3 pb-1 text-[10px] font-medium text-gray-400 uppercase tracking-widest">
			{children}
		</div>
	);
}

interface OptionProps {
	id: string;
	index: number;
	selected: boolean;
	onClick: () => void;
	onMouseMove: () => void;
	children: React.ReactNode;
}

function Option({
	id,
	index,
	selected,
	onClick,
	onMouseMove,
	children,
}: OptionProps) {
	return (
		<div
			id={id}
			role="option"
			aria-selected={selected}
			data-index={index}
			tabIndex={-1}
			onClick={onClick}
			onMouseMove={onMouseMove}
			className={`mx-2 px-3 h-10 rounded-md flex items-center gap-3 cursor-pointer relative ${
				selected ? "bg-indigo-50" : "hover:bg-gray-50"
			}`}
		>
			{selected && (
				<span className="absolute left-0 top-2 bottom-2 w-[3px] rounded-r-full bg-indigo-600" />
			)}
			{children}
		</div>
	);
}

function Hint({
	k,
	children,
}: {
	k: React.ReactNode;
	children: React.ReactNode;
}) {
	return (
		<span className="inline-flex items-center gap-1.5">
			<kbd className="inline-flex items-center justify-center min-w-[18px] h-[18px] px-1 rounded border border-gray-200 bg-gray-50 text-gray-500 font-mono text-[10px] leading-none">
				{k}
			</kbd>
			{children}
		</span>
	);
}
