"use client";

import {
	ArrowRight,
	Download,
	Loader2,
	Plus,
	Send,
	Terminal,
	Trash2,
	X,
} from "lucide-react";
import { useState } from "react";

const variants = [
	{
		name: "Primary",
		bg: "bg-blue-600",
		hover: "hover:bg-blue-700",
		text: "text-white",
		labels: ["Save", "Save Changes", "Create Device"],
	},
	{
		name: "Secondary",
		bg: "bg-gray-100",
		hover: "hover:bg-gray-200",
		text: "text-gray-700",
		labels: ["Cancel", "Clear Selection", "Dismiss"],
	},
	{
		name: "Danger",
		bg: "bg-red-600",
		hover: "hover:bg-red-700",
		text: "text-white",
		labels: ["Delete", "Yank Release", "Remove"],
	},
	{
		name: "Success",
		bg: "bg-green-600",
		hover: "hover:bg-green-700",
		text: "text-white",
		labels: ["Approve", "Publish", "Confirm"],
	},
	{
		name: "Warning",
		bg: "bg-amber-600",
		hover: "hover:bg-amber-700",
		text: "text-white",
		labels: ["Deploy", "Deploy to Selected", "Rollout"],
	},
	{
		name: "Purple",
		bg: "bg-purple-600",
		hover: "hover:bg-purple-700",
		text: "text-white",
		labels: ["Run Command", "Execute", "Send"],
	},
] as const;

const sizes = [
	{ name: "Small", px: "px-2", py: "py-1", text: "text-xs" },
	{ name: "Default", px: "px-3", py: "py-2", text: "text-sm" },
	{ name: "Large", px: "px-4", py: "py-2.5", text: "text-base" },
] as const;

export default function ButtonsPage() {
	const [loadingVariant, setLoadingVariant] = useState<string | null>(null);

	const simulateLoading = (id: string) => {
		setLoadingVariant(id);
		setTimeout(() => setLoadingVariant(null), 2000);
	};

	return (
		<div className="max-w-5xl mx-auto px-6 py-12">
			<h1 className="text-3xl font-bold text-gray-900 mb-12">Button</h1>

				{/* Color Variants */}
				<section className="mb-16">
					<h2 className="text-xs font-medium text-gray-400 uppercase tracking-widest mb-6">
						Variants
					</h2>
					<div className="space-y-5">
						{variants.map((v) => (
							<div key={v.name} className="flex items-center gap-4">
								<span className="text-sm text-gray-500 w-24 shrink-0 text-right">
									{v.name}
								</span>
								<div className="flex flex-wrap items-center gap-3">
									{v.labels.map((label) => (
										<button
											key={label}
											className={`px-3 py-2 text-sm rounded-md cursor-pointer ${v.bg} ${v.hover} ${v.text}`}
										>
											{label}
										</button>
									))}
								</div>
							</div>
						))}
						<div className="flex items-center gap-4">
							<span className="text-sm text-gray-500 w-24 shrink-0 text-right">
								Ghost
							</span>
							<div className="flex flex-wrap items-center gap-4">
								<button className="text-sm text-blue-600 hover:text-blue-800 cursor-pointer">
									View all
								</button>
								<button className="text-sm text-blue-600 hover:underline cursor-pointer">
									device-001
								</button>
								<button className="text-sm text-gray-400 hover:text-gray-600 cursor-pointer">
									Clear filter
								</button>
							</div>
						</div>
					</div>
				</section>

				{/* Sizes */}
				<section className="mb-16">
					<h2 className="text-xs font-medium text-gray-400 uppercase tracking-widest mb-6">
						Sizes
					</h2>
					<div className="space-y-5">
						{variants.map((v) => (
							<div key={v.name} className="flex items-center gap-4">
								<span className="text-sm text-gray-500 w-24 shrink-0 text-right">
									{v.name}
								</span>
								<div className="flex flex-wrap items-end gap-3">
									{sizes.map((s) => (
										<button
											key={s.name}
											className={`${s.px} ${s.py} ${s.text} rounded-md cursor-pointer ${v.bg} ${v.hover} ${v.text}`}
										>
											{s.name}
										</button>
									))}
								</div>
							</div>
						))}
					</div>
				</section>

				{/* Icon Buttons */}
				<section className="mb-16">
					<h2 className="text-xs font-medium text-gray-400 uppercase tracking-widest mb-6">
						With Icons
					</h2>
					<div className="space-y-5">
						<div className="flex items-center gap-4">
							<span className="text-sm text-gray-500 w-24 shrink-0 text-right">
								Icon + Text
							</span>
							<div className="flex flex-wrap items-center gap-3">
								<button className="px-3 py-2 text-sm bg-blue-600 text-white rounded-md hover:bg-blue-700 cursor-pointer flex items-center gap-2">
									<Plus className="w-4 h-4" />
									Add Device
								</button>
								<button className="px-3 py-2 text-sm bg-purple-600 text-white rounded-md hover:bg-purple-700 cursor-pointer flex items-center gap-2">
									<Terminal className="w-4 h-4" />
									Run Command
								</button>
								<button className="px-3 py-2 text-sm bg-green-600 text-white rounded-md hover:bg-green-700 cursor-pointer flex items-center gap-2">
									<Download className="w-4 h-4" />
									Download
								</button>
								<button className="px-3 py-2 text-sm bg-amber-600 text-white rounded-md hover:bg-amber-700 cursor-pointer flex items-center gap-2">
									<Send className="w-4 h-4" />
									Deploy
								</button>
								<button className="px-3 py-2 text-sm bg-red-600 text-white rounded-md hover:bg-red-700 cursor-pointer flex items-center gap-2">
									<Trash2 className="w-4 h-4" />
									Delete
								</button>
							</div>
						</div>
						<div className="flex items-center gap-4">
							<span className="text-sm text-gray-500 w-24 shrink-0 text-right">
								Icon Only
							</span>
							<div className="flex flex-wrap items-center gap-2">
								<button className="p-2 text-gray-400 hover:text-gray-600 cursor-pointer rounded-md hover:bg-gray-100">
									<Plus className="w-4 h-4" />
								</button>
								<button className="p-2 text-gray-400 hover:text-gray-600 cursor-pointer rounded-md hover:bg-gray-100">
									<Trash2 className="w-4 h-4" />
								</button>
								<button className="p-2 text-gray-400 hover:text-gray-600 cursor-pointer rounded-md hover:bg-gray-100">
									<ArrowRight className="w-4 h-4" />
								</button>
								<button className="p-2 text-gray-400 hover:text-gray-600 cursor-pointer rounded-md hover:bg-gray-100">
									<X className="w-4 h-4" />
								</button>
								<button className="p-2 text-gray-400 hover:text-gray-600 cursor-pointer rounded-md hover:bg-gray-100">
									<Terminal className="w-4 h-4" />
								</button>
							</div>
						</div>
					</div>
				</section>

				{/* Toggle / Filter */}
				<section className="mb-16">
					<h2 className="text-xs font-medium text-gray-400 uppercase tracking-widest mb-6">
						Toggle / Filter
					</h2>
					<div className="space-y-5">
						<div className="flex items-center gap-4">
							<span className="text-sm text-gray-500 w-24 shrink-0 text-right">
								Status
							</span>
							<ToggleDemo />
						</div>
						<div className="flex items-center gap-4">
							<span className="text-sm text-gray-500 w-24 shrink-0 text-right">
								Release
							</span>
							<FilterToggleDemo />
						</div>
					</div>
				</section>

				{/* States */}
				<section className="mb-16">
					<h2 className="text-xs font-medium text-gray-400 uppercase tracking-widest mb-6">
						States
					</h2>
					<div className="space-y-5">
						{variants.map((v) => (
							<div key={v.name} className="flex items-center gap-4">
								<span className="text-sm text-gray-500 w-24 shrink-0 text-right">
									{v.name}
								</span>
								<div className="flex flex-wrap items-center gap-3">
									<button
										className={`px-3 py-2 text-sm rounded-md cursor-pointer ${v.bg} ${v.hover} ${v.text}`}
									>
										Default
									</button>
									<button
										disabled
										className={`px-3 py-2 text-sm rounded-md ${v.bg} ${v.text} disabled:opacity-50 disabled:cursor-not-allowed`}
									>
										Disabled
									</button>
									<button
										onClick={() => simulateLoading(v.name)}
										disabled={loadingVariant === v.name}
										className={`px-3 py-2 text-sm rounded-md cursor-pointer ${v.bg} ${v.hover} ${v.text} disabled:opacity-50 disabled:cursor-not-allowed`}
									>
										{loadingVariant === v.name ? (
											<span className="flex items-center gap-2">
												<Loader2 className="w-4 h-4 animate-spin" />
												Loading...
											</span>
										) : (
											"Click to Load"
										)}
									</button>
								</div>
							</div>
						))}
					</div>
				</section>
		</div>
	);
}

function ToggleDemo() {
	const [status, setStatus] = useState<"all" | "online" | "offline">("all");
	const [outdated, setOutdated] = useState(false);

	return (
		<div className="flex flex-wrap items-center gap-3">
			<div className="flex space-x-1">
				<button
					onClick={() => setStatus("all")}
					className={`px-3 py-2 text-sm rounded-md transition-colors cursor-pointer ${
						status === "all"
							? "bg-blue-600 text-white"
							: "bg-gray-100 text-gray-700 hover:bg-gray-200"
					}`}
				>
					All
				</button>
				<button
					onClick={() => setStatus("online")}
					className={`px-3 py-2 text-sm rounded-md transition-colors cursor-pointer ${
						status === "online"
							? "bg-green-600 text-white"
							: "bg-gray-100 text-gray-700 hover:bg-gray-200"
					}`}
				>
					Online
				</button>
				<button
					onClick={() => setStatus("offline")}
					className={`px-3 py-2 text-sm rounded-md transition-colors cursor-pointer ${
						status === "offline"
							? "bg-gray-600 text-white"
							: "bg-gray-100 text-gray-700 hover:bg-gray-200"
					}`}
				>
					Offline
				</button>
			</div>
			<button
				onClick={() => setOutdated(!outdated)}
				className={`px-3 py-2 text-sm rounded-md transition-colors cursor-pointer ${
					outdated
						? "bg-orange-600 text-white"
						: "bg-gray-100 text-gray-700 hover:bg-gray-200"
				}`}
			>
				Outdated
			</button>
		</div>
	);
}

function FilterToggleDemo() {
	const [active, setActive] = useState(false);

	return (
		<div className="flex flex-wrap items-center gap-3">
			<button
				onClick={() => setActive(!active)}
				className={`px-3 py-2 text-sm rounded-md transition-colors cursor-pointer flex items-center gap-2 ${
					active
						? "bg-purple-600 text-white"
						: "bg-gray-100 text-gray-700 hover:bg-gray-200"
				}`}
			>
				Release
			</button>
			{active && (
				<button
					onClick={() => setActive(false)}
					className="text-xs text-gray-400 hover:text-gray-600 cursor-pointer"
				>
					Clear
				</button>
			)}
		</div>
	);
}
