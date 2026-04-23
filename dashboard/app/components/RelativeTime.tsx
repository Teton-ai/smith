import moment from "moment";
import { Tooltip } from "@/app/components/tooltip";

interface RelativeTimeProps {
	date: string | Date | null | undefined;
	className?: string;
}

export const RelativeTime = ({ date, className }: RelativeTimeProps) => {
	if (!date) return null;
	const m = moment(date);
	return (
		<Tooltip content={m.format("MMM D, YYYY h:mm:ss A")} className={className}>
			{m.fromNow()}
		</Tooltip>
	);
};
