import type { ReactNode } from "react";

type Placement = "top" | "bottom" | "left" | "right";

interface TooltipProps {
	content: ReactNode;
	children: ReactNode;
	placement?: Placement;
	className?: string;
}

const placementClasses: Record<Placement, string> = {
	top: "left-1/2 -translate-x-1/2 bottom-full mb-2 group-hover/tt:-translate-y-0.5 translate-y-0.5",
	bottom:
		"left-1/2 -translate-x-1/2 top-full mt-2 group-hover/tt:translate-y-0.5 -translate-y-0.5",
	left: "right-full top-1/2 -translate-y-1/2 mr-2 group-hover/tt:-translate-x-0.5 translate-x-0.5",
	right:
		"left-full top-1/2 -translate-y-1/2 ml-2 group-hover/tt:translate-x-0.5 -translate-x-0.5",
};

const arrowClasses: Record<Placement, string> = {
	top: "left-1/2 -translate-x-1/2 top-full -mt-1",
	bottom: "left-1/2 -translate-x-1/2 bottom-full -mb-1",
	left: "top-1/2 -translate-y-1/2 left-full -ml-1",
	right: "top-1/2 -translate-y-1/2 right-full -mr-1",
};

export const Tooltip = ({
	content,
	children,
	placement = "top",
	className,
}: TooltipProps) => {
	return (
		<span className={`relative inline-block group/tt ${className ?? ""}`}>
			{children}
			<span
				role="tooltip"
				className={`pointer-events-none absolute ${placementClasses[placement]} whitespace-nowrap rounded-md bg-gray-900 px-2 py-1 text-xs font-normal text-white shadow-lg opacity-0 group-hover/tt:opacity-100 transition-[opacity,transform] duration-150 ease-out z-50`}
			>
				{content}
				<span
					className={`absolute ${arrowClasses[placement]} w-2 h-2 bg-gray-900 rotate-45`}
				/>
			</span>
		</span>
	);
};
