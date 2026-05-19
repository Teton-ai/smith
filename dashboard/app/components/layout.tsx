import {
	AppWindow,
	Layers,
	MousePointerClick,
	Palette,
	SlidersHorizontal,
	Type,
} from "lucide-react";
import { Outlet } from "react-router";
import Sidebar from "@/app/components/sidebar";

const componentPages = [
	{ path: "/components/buttons", label: "Button", icon: MousePointerClick },
	{ path: "/components/overlay", label: "Overlay", icon: Layers },
	{ path: "/components/input", label: "Input", icon: SlidersHorizontal },
	{ path: "/components/text", label: "Text", icon: Type },
	{ path: "/components/color", label: "Color", icon: Palette },
	{ path: "/components/controls", label: "Controls", icon: AppWindow },
];

export default function ComponentsLayout() {
	return (
		<div className="flex h-screen overflow-hidden bg-gray-50">
			<Sidebar items={componentPages} className="bg-white" />
			<main className="flex-1 min-w-0 overflow-auto mt-14 md:mt-0">
				<Outlet />
			</main>
		</div>
	);
}
