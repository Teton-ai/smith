import type { ReactNode } from "react";
import { useLocation } from "react-router";
import type { Device } from "@/app/api-client";
import { BackLink, TabNav } from "@/app/components/ui";
import DeviceHeader from "./DeviceHeader";

export type DeviceTab = "overview" | "commands" | "services";

/**
 * Shared scaffold for the device detail sub-pages (Overview / Commands /
 * Services). Renders the back link, device header and tab bar identically on
 * every tab so switching between them doesn't shift the layout.
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

	const base = "flex-1 px-4 sm:px-6 lg:px-8 py-6 space-y-6";

	return (
		<div
			className={
				fill
					? `${base} flex flex-col overflow-hidden`
					: `${base} overflow-y-auto`
			}
		>
			<BackLink to={back}>Back to Devices</BackLink>

			{device && <DeviceHeader device={device} serial={serial} />}

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

			{fill ? (
				<div className="flex-1 overflow-hidden min-h-0">{children}</div>
			) : (
				children
			)}
		</div>
	);
}
