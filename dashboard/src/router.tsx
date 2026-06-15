import { lazy, Suspense } from "react";
import { createBrowserRouter, Navigate } from "react-router";

const LoginPage = lazy(() => import("@/app/page"));
const PrivateLayout = lazy(() => import("@/app/(private)/layout"));
const Dashboard = lazy(() => import("@/app/(private)/dashboard/page"));
const Devices = lazy(() => import("@/app/(private)/devices/page"));
const DeviceDetail = lazy(
	() => import("@/app/(private)/devices/[serial]/page"),
);
const DeviceServices = lazy(
	() => import("@/app/(private)/devices/[serial]/services/page"),
);
const DeviceCommands = lazy(
	() => import("@/app/(private)/devices/[serial]/commands/page"),
);
const DeviceAudit = lazy(
	() => import("@/app/(private)/devices/[serial]/audit/page"),
);
const Distributions = lazy(() => import("@/app/(private)/distributions/page"));
const DistributionDetail = lazy(
	() => import("@/app/(private)/distributions/[id]/page"),
);
const ReleaseDetail = lazy(() => import("@/app/(private)/releases/[id]/page"));
const ReleaseDeployment = lazy(
	() => import("@/app/(private)/releases/[id]/deployment/page"),
);
const Commands = lazy(() => import("@/app/(private)/commands/page"));
const Recipes = lazy(() => import("@/app/(private)/recipes/page"));
const IpAddresses = lazy(() => import("@/app/(private)/ip-addresses/page"));
const Modems = lazy(() => import("@/app/(private)/modems/page"));
const NetworkTesting = lazy(
	() => import("@/app/(private)/network-testing/page"),
);

const ComponentsLayout = lazy(() => import("@/app/components/layout"));
const ButtonsPage = lazy(() => import("@/app/components/buttons/page"));
const ColorPage = lazy(() => import("@/app/components/color/page"));
const ControlsPage = lazy(() => import("@/app/components/controls/page"));
const InputPage = lazy(() => import("@/app/components/input/page"));
const OverlayPage = lazy(() => import("@/app/components/overlay/page"));
const TextPage = lazy(() => import("@/app/components/text/page"));

const Fallback = () => null;

function withSuspense(node: React.ReactNode) {
	return <Suspense fallback={<Fallback />}>{node}</Suspense>;
}

export const router = createBrowserRouter([
	{
		path: "/",
		element: withSuspense(<LoginPage />),
	},
	{
		element: withSuspense(<PrivateLayout />),
		children: [
			{ path: "/dashboard", element: withSuspense(<Dashboard />) },
			{ path: "/devices", element: withSuspense(<Devices />) },
			{ path: "/devices/:serial", element: withSuspense(<DeviceDetail />) },
			{
				path: "/devices/:serial/services",
				element: withSuspense(<DeviceServices />),
			},
			{
				path: "/devices/:serial/commands",
				element: withSuspense(<DeviceCommands />),
			},
			{
				path: "/devices/:serial/audit",
				element: withSuspense(<DeviceAudit />),
			},
			{ path: "/distributions", element: withSuspense(<Distributions />) },
			{
				path: "/distributions/:id",
				element: withSuspense(<DistributionDetail />),
			},
			{ path: "/releases/:id", element: withSuspense(<ReleaseDetail />) },
			{
				path: "/releases/:id/deployment",
				element: withSuspense(<ReleaseDeployment />),
			},
			{ path: "/commands", element: withSuspense(<Commands />) },
			{ path: "/recipes", element: withSuspense(<Recipes />) },
			{ path: "/ip-addresses", element: withSuspense(<IpAddresses />) },
			{ path: "/modems", element: withSuspense(<Modems />) },
			{ path: "/network-testing", element: withSuspense(<NetworkTesting />) },
		],
	},
	{
		path: "/components",
		element: <Navigate to="/components/buttons" replace />,
	},
	{
		element: withSuspense(<ComponentsLayout />),
		children: [
			{ path: "/components/buttons", element: withSuspense(<ButtonsPage />) },
			{ path: "/components/color", element: withSuspense(<ColorPage />) },
			{ path: "/components/controls", element: withSuspense(<ControlsPage />) },
			{ path: "/components/input", element: withSuspense(<InputPage />) },
			{ path: "/components/overlay", element: withSuspense(<OverlayPage />) },
			{ path: "/components/text", element: withSuspense(<TextPage />) },
		],
	},
]);
