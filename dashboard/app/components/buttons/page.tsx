"use client";

import { ArrowRight, Plus, Terminal, Trash2, X } from "lucide-react";
import { useState } from "react";
import { Button, IconButton } from "../button";

export default function ButtonsPage() {
	const [loading, setLoading] = useState(false);

	const simulateLoading = () => {
		setLoading(true);
		setTimeout(() => setLoading(false), 2000);
	};

	return (
		<div className="max-w-5xl mx-auto px-6 py-12">
			<h1 className="text-3xl font-bold text-gray-900 mb-12">Button</h1>

			{/* Variants */}
			<section className="mb-16">
				<h2 className="text-xs font-medium text-gray-400 uppercase tracking-widest mb-6">
					Variants
				</h2>
				<div className="flex flex-wrap items-center gap-3">
					<Button variant="primary">Primary</Button>
					<Button variant="secondary">Secondary</Button>
					<Button variant="danger">Danger</Button>
					<Button variant="success">Success</Button>
					<Button variant="warning">Warning</Button>
					<Button variant="purple">Purple</Button>
					<Button variant="ghost">Ghost</Button>
				</div>
			</section>

			{/* Icon Only */}
			<section className="mb-16">
				<h2 className="text-xs font-medium text-gray-400 uppercase tracking-widest mb-6">
					Icon Only
				</h2>
				<div className="flex flex-wrap items-center gap-2">
					<IconButton icon={<Plus className="w-4 h-4" />} />
					<IconButton icon={<Trash2 className="w-4 h-4" />} />
					<IconButton icon={<ArrowRight className="w-4 h-4" />} />
					<IconButton icon={<X className="w-4 h-4" />} />
					<IconButton icon={<Terminal className="w-4 h-4" />} />
				</div>
			</section>

			{/* States */}
			<section className="mb-16">
				<h2 className="text-xs font-medium text-gray-400 uppercase tracking-widest mb-6">
					States
				</h2>
				<div className="flex flex-wrap items-center gap-3">
					<Button>Default</Button>
					<Button disabled>Disabled</Button>
					<Button loading={loading} onClick={simulateLoading}>
						{loading ? "Loading..." : "Click to Load"}
					</Button>
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
		</div>
	);
}

function ToggleDemo() {
	const [status, setStatus] = useState<"all" | "online" | "offline">("all");
	const [outdated, setOutdated] = useState(false);

	return (
		<div className="flex flex-wrap items-center gap-3">
			<div className="flex space-x-1">
				<Button
					variant={status === "all" ? "primary" : "secondary"}
					onClick={() => setStatus("all")}
				>
					All
				</Button>
				<Button
					variant={status === "online" ? "success" : "secondary"}
					onClick={() => setStatus("online")}
				>
					Online
				</Button>
				<Button
					variant={status === "offline" ? "secondary" : "secondary"}
					className={
						status === "offline"
							? "bg-gray-600 hover:bg-gray-700 text-white"
							: ""
					}
					onClick={() => setStatus("offline")}
				>
					Offline
				</Button>
			</div>
			<Button
				variant={outdated ? "warning" : "secondary"}
				className={
					outdated ? "bg-orange-600 hover:bg-orange-700" : ""
				}
				onClick={() => setOutdated(!outdated)}
			>
				Outdated
			</Button>
		</div>
	);
}

function FilterToggleDemo() {
	const [active, setActive] = useState(false);

	return (
		<div className="flex flex-wrap items-center gap-3">
			<Button
				variant={active ? "purple" : "secondary"}
				onClick={() => setActive(!active)}
			>
				Release
			</Button>
			{active && (
				<Button variant="ghost" onClick={() => setActive(false)}>
					Clear
				</Button>
			)}
		</div>
	);
}
