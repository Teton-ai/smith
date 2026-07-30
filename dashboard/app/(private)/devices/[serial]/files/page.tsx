import {
	AlertBanner,
	Badge,
	Breadcrumbs,
	Card,
	ListRow,
	SearchInput,
	Toast,
	type ToastState,
} from "@teton/smith-ui";
import { FolderOpen, Loader2, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useParams, useSearchParams } from "react-router";
import { useGetDeviceInfo } from "@/app/api-client";
import { DeviceDetailLayout } from "../DeviceDetailLayout";
import { FileTable } from "./FileTable";
import { humanBytes, joinPath, parentPath, pathSegments } from "./formatters";
import {
	type DirEntry,
	FileOpError,
	type FileOpErrorCode,
	type Listing,
	useFileSession,
} from "./useFileSession";

/** Starting points an operator almost always wants; everything else is reached
 *  by navigating. */
const PINNED_ROOTS = ["/", "/var/log", "/etc", "/opt", "/home", "/tmp"];

const ERROR_COPY: Record<FileOpErrorCode, string> = {
	NotFound: "No longer exists",
	PermissionDenied: "The daemon could not read this",
	NotADirectory: "Not a directory",
	NotRegularFile: "Not a regular file",
	TooLarge: "Larger than the 512 MB download limit",
	TooManyOpenFiles: "Too many downloads in flight — try again shortly",
	Io: "The device reported an I/O error",
	Timeout: "The device did not respond in time",
};

function describe(err: unknown): string {
	if (err instanceof FileOpError) return ERROR_COPY[err.code] ?? err.message;
	return err instanceof Error ? err.message : String(err);
}

const FilesPage = () => {
	const params = useParams();
	const serial = params.serial as string;
	const [searchParams, setSearchParams] = useSearchParams();

	const { data: device } = useGetDeviceInfo(serial);
	const { status, error, elapsed, list, download, retry } =
		useFileSession(serial);

	const path = searchParams.get("path") ?? "/";
	const filter = searchParams.get("q") ?? "";

	const [listing, setListing] = useState<Listing | null>(null);
	const [loadingPath, setLoadingPath] = useState<string | null>(null);
	const [listError, setListError] = useState<string | null>(null);
	const [downloading, setDownloading] = useState<string | null>(null);
	const [toast, setToast] = useState<ToastState | null>(null);

	// Guards against an earlier, slower listing overwriting a newer one when the
	// user clicks through directories quickly.
	const requestRef = useRef(0);

	const loadPath = useCallback(
		async (target: string) => {
			const request = ++requestRef.current;
			setLoadingPath(target);
			setListError(null);
			try {
				const result = await list(target);
				if (requestRef.current !== request) return;
				setListing(result);
			} catch (err) {
				if (requestRef.current !== request) return;
				setListError(describe(err));
			} finally {
				if (requestRef.current === request) setLoadingPath(null);
			}
		},
		[list],
	);

	useEffect(() => {
		if (status !== "ready") return;
		loadPath(path);
	}, [status, path, loadPath]);

	const navigate = useCallback(
		(target: string) => {
			const next = new URLSearchParams(searchParams);
			next.set("path", target);
			next.delete("q");
			// Push, not replace: Back should walk up the directory tree, which is
			// what a file browser is expected to do.
			setSearchParams(next);
		},
		[searchParams, setSearchParams],
	);

	const setFilter = useCallback(
		(value: string) => {
			const next = new URLSearchParams(searchParams);
			if (value) next.set("q", value);
			else next.delete("q");
			// Replace: typing shouldn't flood history.
			setSearchParams(next, { replace: true });
		},
		[searchParams, setSearchParams],
	);

	const handleDownload = useCallback(
		async (entry: DirEntry) => {
			setDownloading(entry.name);
			try {
				const ready = await download(
					joinPath(listing?.path ?? path, entry.name),
				);
				// The link points straight at the CDN, so the browser's own download
				// manager owns progress from here — no buffering in the tab.
				const anchor = document.createElement("a");
				anchor.href = ready.url;
				anchor.download = ready.name;
				document.body.appendChild(anchor);
				anchor.click();
				anchor.remove();
				setToast({
					message: `Downloading ${ready.name} (${humanBytes(ready.size)})`,
					type: "success",
				});
			} catch (err) {
				setToast({ message: describe(err), type: "error" });
			} finally {
				setDownloading(null);
			}
		},
		[download, listing?.path, path],
	);

	const currentPath = listing?.path ?? path;
	const segments = useMemo(() => pathSegments(currentPath), [currentPath]);

	const visible = useMemo(() => {
		if (!listing) return [];
		if (!filter) return listing.entries;
		const needle = filter.toLowerCase();
		return listing.entries.filter((entry) =>
			entry.name.toLowerCase().includes(needle),
		);
	}, [listing, filter]);

	if (status === "connecting") {
		return (
			<DeviceDetailLayout
				serial={serial}
				device={device}
				activeTab="files"
				fill
			>
				<Card className="h-full flex items-center justify-center">
					<div className="text-center">
						<Loader2 className="w-8 h-8 text-blue-500 animate-spin mx-auto mb-4" />
						<p className="text-gray-900 font-medium">
							Waiting for device to connect…
						</p>
						<p className="text-gray-500 text-sm mt-1">
							Devices check in every ~20 seconds
						</p>
						<p className="text-gray-400 text-sm mt-3 tabular-nums">
							{elapsed}s
						</p>
					</div>
				</Card>
			</DeviceDetailLayout>
		);
	}

	if (status === "error" || status === "closed") {
		return (
			<DeviceDetailLayout
				serial={serial}
				device={device}
				activeTab="files"
				fill
			>
				<Card className="h-full flex items-center justify-center">
					<div className="text-center max-w-md px-6">
						<p className="text-gray-900 font-medium">
							{status === "error"
								? "Could not browse this device"
								: "Session ended"}
						</p>
						{error && <p className="text-gray-500 text-sm mt-1">{error}</p>}
						<button
							type="button"
							onClick={retry}
							className="mt-4 inline-flex items-center gap-2 px-3 py-1.5 text-sm rounded-md border border-gray-300 text-gray-700 hover:bg-gray-50 transition-colors cursor-pointer"
						>
							<RefreshCw className="w-4 h-4" />
							Reconnect
						</button>
					</div>
				</Card>
			</DeviceDetailLayout>
		);
	}

	return (
		<DeviceDetailLayout serial={serial} device={device} activeTab="files" fill>
			<div className="flex gap-4 h-full min-h-0">
				<div className="w-48 flex-shrink-0">
					<Card className="overflow-hidden">
						<div className="divide-y divide-gray-100">
							{PINNED_ROOTS.map((root) => (
								<ListRow
									key={root}
									hover=""
									className={
										currentPath === root
											? "bg-blue-50 border-l-2 border-l-blue-500"
											: "border-l-2 border-l-transparent"
									}
								>
									<button
										type="button"
										onClick={() => navigate(root)}
										className="font-mono text-sm text-gray-700 hover:text-blue-600 truncate cursor-pointer"
									>
										{root}
									</button>
								</ListRow>
							))}
						</div>
					</Card>
				</div>

				<div className="flex-1 min-w-0 flex flex-col">
					<Card className="flex-1 flex flex-col overflow-hidden min-h-0">
						<div className="flex items-center justify-between gap-4 px-4 py-2.5 border-b border-gray-100 bg-gray-50">
							<Breadcrumbs
								segments={segments}
								onNavigate={(index) => navigate(segments[index].key)}
								className="flex-1"
							/>
							<div className="flex items-center gap-2 flex-shrink-0">
								{listing?.truncated && (
									<Badge variant="yellow">First 5,000 entries</Badge>
								)}
								<SearchInput
									value={filter}
									onChange={setFilter}
									placeholder="Filter…"
								/>
							</div>
						</div>

						{loadingPath && !listing && (
							<div className="flex-1 flex items-center justify-center">
								<Loader2 className="w-6 h-6 text-gray-300 animate-spin" />
							</div>
						)}

						{listError && (
							<div className="p-4">
								<AlertBanner
									tone="amber"
									title="Could not open this directory"
									action={
										<button
											type="button"
											onClick={() => loadPath(currentPath)}
											className="inline-flex items-center gap-1.5 px-2.5 py-1 text-sm rounded-md border border-gray-300 text-gray-700 hover:bg-gray-50 transition-colors cursor-pointer"
										>
											<RefreshCw className="w-3.5 h-3.5" />
											Retry
										</button>
									}
								>
									{listError}
								</AlertBanner>
							</div>
						)}

						{listing && !listError && (
							<div className="flex-1 overflow-y-auto min-h-0">
								{visible.length === 0 ? (
									<div className="flex items-center justify-center h-64 text-gray-400">
										<div className="text-center">
											<FolderOpen className="w-8 h-8 mx-auto mb-2 opacity-50" />
											<p className="text-sm">
												{filter
													? "Nothing matches that filter"
													: "This directory is empty"}
											</p>
										</div>
									</div>
								) : (
									<FileTable
										entries={visible}
										atRoot={currentPath === "/"}
										downloading={downloading}
										stale={loadingPath !== null}
										onOpenDir={(name) => navigate(joinPath(currentPath, name))}
										onNavigateUp={() => navigate(parentPath(currentPath))}
										onDownload={handleDownload}
									/>
								)}
							</div>
						)}
					</Card>
				</div>
			</div>

			<Toast toast={toast} onClose={() => setToast(null)} />
		</DeviceDetailLayout>
	);
};

export default FilesPage;
