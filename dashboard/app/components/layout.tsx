"use client";

import {
	AppWindow,
	Layers,
	MousePointerClick,
	Palette,
	SlidersHorizontal,
	Type,
} from "lucide-react";
import type React from "react";
import Sidebar from "@/app/components/sidebar";

const componentPages = [
	{ path: "/components/buttons", label: "Button", icon: MousePointerClick },
	{ path: "/components/overlay", label: "Overlay", icon: Layers },
	{ path: "/components/input", label: "Input", icon: SlidersHorizontal },
	{ path: "/components/text", label: "Text", icon: Type },
	{ path: "/components/color", label: "Color", icon: Palette },
	{ path: "/components/controls", label: "Controls", icon: AppWindow },
];

export default function ComponentsLayout({
	children,
}: {
	children: React.ReactNode;
}) {
	return (
		<Sidebar items={componentPages} className="bg-white">
			{children}
		</Sidebar>
	);
}
