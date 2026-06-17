// Shared color themes for section cards / colored list groups.
// Full class strings (no dynamic concatenation) so Tailwind keeps them.

export interface SectionTheme {
	/** Header bar background + text color */
	header: string;
	/** Row hover background */
	hover: string;
	/** Count pill colors */
	badge: string;
}

export const SECTION_THEMES = {
	blue: {
		header: "bg-blue-50 text-blue-800",
		hover: "hover:bg-blue-50",
		badge: "bg-blue-100 text-blue-700",
	},
	purple: {
		header: "bg-purple-50 text-purple-800",
		hover: "hover:bg-purple-50",
		badge: "bg-purple-100 text-purple-700",
	},
	rose: {
		header: "bg-rose-50 text-rose-800",
		hover: "hover:bg-rose-50",
		badge: "bg-rose-100 text-rose-700",
	},
	yellow: {
		header: "bg-yellow-50 text-yellow-800",
		hover: "hover:bg-yellow-50",
		badge: "bg-yellow-100 text-yellow-700",
	},
	orange: {
		header: "bg-orange-50 text-orange-800",
		hover: "hover:bg-orange-50",
		badge: "bg-orange-100 text-orange-700",
	},
	red: {
		header: "bg-red-50 text-red-800",
		hover: "hover:bg-red-50",
		badge: "bg-red-100 text-red-700",
	},
	green: {
		header: "bg-green-50 text-green-800",
		hover: "hover:bg-green-50",
		badge: "bg-green-100 text-green-700",
	},
	gray: {
		header: "bg-gray-50 text-gray-700",
		hover: "hover:bg-gray-50",
		badge: "bg-gray-200 text-gray-700",
	},
} satisfies Record<string, SectionTheme>;

export type SectionThemeName = keyof typeof SECTION_THEMES;

export type IconComponent = React.ComponentType<{ className?: string }>;
