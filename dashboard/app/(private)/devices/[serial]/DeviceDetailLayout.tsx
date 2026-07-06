import { TabNav } from "@teton/smith-ui";
import type { ReactNode } from "react";
import { useLocation } from "react-router";
import type { Device } from "@/app/api-client";
import DeviceHeader from "./DeviceHeader";

export type DeviceTab = "overview" | "commands" | "services";

/**
 * Shared scaffold for the device detail sub-pages (Overview / Commands /
 * Services). Renders the device header (with its back-link breadcrumb) and
 * tab bar identically on every tab so switching between them doesn't shift
 * the layout.
 *
 * Pass `fill` for pages whose content scrolls internally (e.g. Commands): the
 * content area becomes a flex child that fills the remaining height instead of
 * letting the whole page scroll.
 */
export function DeviceDetailLayout({
	serial,
	device,
	activeTab,
	fill = false,
	children,
}: {
	serial: string;
	device?: Device;
	activeTab: DeviceTab;
	fill?: boolean;
	children: ReactNode;
}) {
	const back =
		(useLocation().state as { back?: string } | null)?.back ?? "/devices";
	const backState = { back };

	const base = "flex-1 px-4 sm:px-6 lg:px-8 py-6";

	return (
		<div
			className={
				fill
					? `${base} flex flex-col overflow-hidden`
					: `${base} overflow-y-auto`
			}
		>
			<DeviceHeader device={device} serial={serial} back={back} />

			<div className="mt-6">
				<TabNav
					items={[
						{
							label: "Overview",
							to: `/devices/${serial}`,
							active: activeTab === "overview",
							state: backState,
						},
						{
							label: "Commands",
							to: `/devices/${serial}/commands`,
							active: activeTab === "commands",
							state: backState,
						},
						{
							label: "Services",
							to: `/devices/${serial}/services`,
							active: activeTab === "services",
							state: backState,
						},
					]}
				/>
			</div>

			{fill ? (
				<div className="flex-1 overflow-hidden min-h-0 mt-6 space-y-6">
					{children}
				</div>
			) : (
				<div className="mt-6 space-y-6">{children}</div>
			)}
		</div>
	);
}
