import {
	CornerLeftUp,
	Download,
	File as FileIcon,
	Folder,
	HelpCircle,
	Link2,
	Loader2,
} from "lucide-react";
import { formatMtime, humanBytes, modeToRwx } from "./formatters";
import type { DirEntry, FileKind } from "./useFileSession";

const KIND_ICON = {
	Dir: Folder,
	File: FileIcon,
	Symlink: Link2,
	Other: HelpCircle,
} satisfies Record<FileKind, typeof Folder>;

export function FileTable({
	entries,
	atRoot,
	downloading,
	stale,
	onOpenDir,
	onNavigateUp,
	onDownload,
}: {
	entries: DirEntry[];
	atRoot: boolean;
	/** op currently being staged, so only that row shows a spinner. */
	downloading: string | null;
	/** Dim the table while the next listing is in flight, rather than blanking
	 *  it — at these latencies a spinner would flash and read as broken. */
	stale: boolean;
	onOpenDir: (name: string) => void;
	onNavigateUp: () => void;
	onDownload: (entry: DirEntry) => void;
}) {
	return (
		<div
			className={`overflow-x-auto transition-opacity ${stale ? "opacity-60 pointer-events-none" : ""}`}
		>
			<table className="min-w-full divide-y divide-gray-200">
				<thead className="bg-gray-50">
					<tr>
						<th className="px-4 py-2 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
							Name
						</th>
						<th className="px-4 py-2 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">
							Size
						</th>
						<th className="px-4 py-2 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
							Modified
						</th>
						<th className="px-4 py-2 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
							Mode
						</th>
						<th className="px-4 py-2 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">
							Owner
						</th>
						<th className="px-4 py-2 w-12" />
					</tr>
				</thead>
				<tbody className="bg-white divide-y divide-gray-100">
					{!atRoot && (
						<tr className="hover:bg-gray-50">
							<td colSpan={6} className="px-4 py-2">
								<button
									type="button"
									onClick={onNavigateUp}
									className="flex items-center gap-2 text-sm text-gray-500 hover:text-gray-900 cursor-pointer"
								>
									<CornerLeftUp className="w-4 h-4" />
									<span className="font-mono">..</span>
								</button>
							</td>
						</tr>
					)}

					{entries.map((entry) => (
						<FileRow
							key={entry.name}
							entry={entry}
							downloading={downloading === entry.name}
							onOpenDir={onOpenDir}
							onDownload={onDownload}
						/>
					))}
				</tbody>
			</table>
		</div>
	);
}

function FileRow({
	entry,
	downloading,
	onOpenDir,
	onDownload,
}: {
	entry: DirEntry;
	downloading: boolean;
	onOpenDir: (name: string) => void;
	onDownload: (entry: DirEntry) => void;
}) {
	const Icon = KIND_ICON[entry.kind] ?? HelpCircle;

	// Symlinks are navigable: the daemon canonicalizes before acting, so
	// following one is safe and is what an operator expects.
	const navigable =
		entry.reachable && (entry.kind === "Dir" || entry.kind === "Symlink");
	const downloadable = entry.reachable && entry.kind !== "Dir";

	const downloadTitle = !entry.reachable
		? "Filename is not valid UTF-8, so it cannot be addressed"
		: entry.kind === "Dir"
			? "Directories cannot be downloaded"
			: entry.kind === "Other"
				? "Not a regular file"
				: "Download";

	return (
		<tr className="hover:bg-gray-50">
			<td className="px-4 py-2 whitespace-nowrap">
				<div className="flex items-center gap-2 min-w-0">
					<Icon
						className={`w-4 h-4 flex-shrink-0 ${
							entry.kind === "Dir" ? "text-blue-500" : "text-gray-400"
						}`}
					/>
					{navigable ? (
						<button
							type="button"
							onClick={() => onOpenDir(entry.name)}
							className="font-mono text-sm text-gray-900 hover:text-blue-600 truncate cursor-pointer"
						>
							{entry.name}
						</button>
					) : (
						<span
							className={`font-mono text-sm truncate ${
								entry.reachable ? "text-gray-900" : "text-gray-400 italic"
							}`}
							title={entry.reachable ? undefined : downloadTitle}
						>
							{entry.name}
						</span>
					)}
					{entry.symlink_target && (
						<span
							className="text-xs text-gray-400 truncate"
							title={entry.symlink_target}
						>
							→ {entry.symlink_target}
						</span>
					)}
				</div>
			</td>
			<td className="px-4 py-2 whitespace-nowrap text-right text-sm text-gray-500 tabular-nums">
				{entry.kind === "Dir" ? "—" : humanBytes(entry.size)}
			</td>
			<td className="px-4 py-2 whitespace-nowrap text-sm text-gray-500">
				{formatMtime(entry.mtime)}
			</td>
			<td className="px-4 py-2 whitespace-nowrap font-mono text-xs text-gray-400">
				{modeToRwx(entry.mode, entry.kind)}
			</td>
			<td className="px-4 py-2 whitespace-nowrap text-sm text-gray-500 tabular-nums">
				{entry.uid}:{entry.gid}
			</td>
			<td className="px-4 py-2 whitespace-nowrap text-right">
				<button
					type="button"
					disabled={!downloadable || downloading}
					onClick={() => onDownload(entry)}
					title={downloadTitle}
					className="p-1.5 rounded text-gray-400 enabled:hover:text-blue-600 enabled:hover:bg-blue-50 disabled:opacity-30 disabled:cursor-not-allowed transition-colors cursor-pointer"
				>
					{downloading ? (
						<Loader2 className="w-4 h-4 animate-spin" />
					) : (
						<Download className="w-4 h-4" />
					)}
				</button>
			</td>
		</tr>
	);
}
