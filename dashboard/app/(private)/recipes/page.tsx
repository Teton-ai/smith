"use client";

import { useQueryClient } from "@tanstack/react-query";
import { Button, Card, SearchInput } from "@teton/smith-ui";
import { Plus, ScrollText, Send, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import {
	buildCommand,
	CommandRow,
	commandIsValid,
	type EditableCommand,
	emptyCommand,
	parseCommand,
} from "@/app/(private)/commands/shared";
import {
	type CommandRecipe,
	type Device,
	getGetRecipesQueryKey,
	type RecipeInput,
	useCreateRecipe,
	useDeleteRecipe,
	useGetDevices,
	useGetRecipes,
	useIssueCommandsToDevices,
	useUpdateRecipe,
} from "@/app/api-client";
import { Modal } from "@/app/components/modal";
import { RelativeTime } from "@/app/components/RelativeTime";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

// The client mutator returns the raw response body, so query `data` is the
// array at runtime even though the generated types wrap it. Normalize here.
function asArray<T>(data: unknown): T[] {
	return Array.isArray(data) ? (data as T[]) : [];
}

function commandsFromRecipe(recipe: CommandRecipe | null): EditableCommand[] {
	if (!recipe) return [emptyCommand()];
	const parsed = asArray<Record<string, unknown>>(recipe.commands).map((c) =>
		parseCommand(c.command, Boolean(c.continue_on_error)),
	);
	return parsed.length > 0 ? parsed : [emptyCommand()];
}

const fieldClass =
	"w-full px-3 py-2 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-gray-900 placeholder-gray-400 text-sm";

// ---------------------------------------------------------------------------
// Right panel: view / edit a recipe
// ---------------------------------------------------------------------------

function RecipeEditor({
	recipe,
	onSaved,
	onDeleteRequest,
	onTrigger,
}: {
	recipe: CommandRecipe | null;
	onSaved: (name: string) => void;
	onDeleteRequest: () => void;
	onTrigger: () => void;
}) {
	const queryClient = useQueryClient();
	const [name, setName] = useState(recipe?.name ?? "");
	const [description, setDescription] = useState(recipe?.description ?? "");
	const [commands, setCommands] = useState<EditableCommand[]>(
		commandsFromRecipe(recipe),
	);
	const [error, setError] = useState<string | null>(null);

	const invalidate = () =>
		queryClient.invalidateQueries({ queryKey: getGetRecipesQueryKey() });

	const onError = () => setError("Failed to save recipe. Is the name unique?");

	const createMut = useCreateRecipe({
		mutation: {
			onSuccess: () => {
				invalidate();
				onSaved(name.trim());
			},
			onError,
		},
	});
	const updateMut = useUpdateRecipe({
		mutation: {
			onSuccess: () => {
				invalidate();
				onSaved(name.trim());
			},
			onError,
		},
	});

	const isPending = createMut.isPending || updateMut.isPending;

	const isValid =
		name.trim().length > 0 &&
		commands.length > 0 &&
		commands.every(commandIsValid);

	// Only build once every row is valid: buildCommand's Number() conversions on
	// an in-progress, not-yet-valid row (e.g. empty duration_minutes) would
	// otherwise produce NaN, which JSON.stringify silently turns into null in
	// the isDirty comparison below.
	const builtCommands = isValid
		? commands.map((ec) => ({
				command: buildCommand(ec),
				continue_on_error: ec.continue_on_error,
			}))
		: [];

	// Whether the form differs from the saved recipe. A new (unsaved) recipe is
	// always considered dirty so it can be created.
	const isDirty = recipe
		? name.trim() !== recipe.name ||
			(description.trim() || "") !== (recipe.description ?? "") ||
			JSON.stringify(builtCommands) !==
				JSON.stringify(
					asArray<Record<string, unknown>>(recipe.commands).map((c) => ({
						command: c.command,
						continue_on_error: Boolean(c.continue_on_error),
					})),
				)
		: true;

	const handleSave = () => {
		if (!isValid || !isDirty) return;
		setError(null);
		const payload: RecipeInput = {
			name: name.trim(),
			description: description.trim() || undefined,
			commands: builtCommands.map((c) => ({
				id: -1,
				...c,
			})) as unknown as RecipeInput["commands"],
		};
		if (recipe) {
			updateMut.mutate({ recipeId: recipe.id, data: payload });
		} else {
			createMut.mutate({ data: payload });
		}
	};

	return (
		<div className="h-full flex flex-col overflow-hidden">
			{/* Body */}
			<div className="flex-1 overflow-y-auto overflow-x-hidden px-4 py-4 space-y-4">
				{error && (
					<div className="bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg p-3">
						{error}
					</div>
				)}
				<div>
					<label className="block text-sm font-medium text-gray-700 mb-1">
						Name
					</label>
					<input
						type="text"
						value={name}
						onChange={(e) => setName(e.target.value)}
						placeholder="e.g., Health check"
						className={fieldClass}
					/>
				</div>
				<div>
					<label className="block text-sm font-medium text-gray-700 mb-1">
						Description <span className="text-gray-400">(optional)</span>
					</label>
					<input
						type="text"
						value={description}
						onChange={(e) => setDescription(e.target.value)}
						placeholder="What does this recipe do?"
						className={fieldClass}
					/>
				</div>

				<div>
					<div className="flex items-center justify-between mb-2">
						<label className="block text-sm font-medium text-gray-700">
							Commands
						</label>
						<Button
							variant="soft"
							tone="blue"
							size="sm"
							icon={<Plus className="w-4 h-4" />}
							onClick={() => setCommands((c) => [...c, emptyCommand()])}
						>
							Add command
						</Button>
					</div>
					<div className="space-y-2">
						{commands.map((c, i) => (
							<CommandRow
								key={i}
								command={c}
								index={i}
								onChange={(next) =>
									setCommands((cs) => cs.map((x, j) => (j === i ? next : x)))
								}
								onRemove={() =>
									setCommands((cs) => cs.filter((_, j) => j !== i))
								}
							/>
						))}
					</div>
				</div>
			</div>

			{/* Footer */}
			<div className="flex items-center justify-between px-4 py-3 border-t border-gray-200/80 shrink-0">
				{recipe ? (
					<Button
						variant="soft"
						tone="red"
						icon={<Trash2 className="w-4 h-4" />}
						onClick={onDeleteRequest}
					>
						Delete
					</Button>
				) : (
					<span />
				)}
				<div className="flex items-center gap-3">
					{recipe && (
						<Button
							variant="soft"
							tone="purple"
							icon={<Send className="w-4 h-4" />}
							onClick={onTrigger}
						>
							Trigger
						</Button>
					)}
					{(!recipe || isDirty) && (
						<Button
							variant="solid"
							tone="blue"
							loading={isPending}
							disabled={!isValid}
							onClick={handleSave}
						>
							{recipe ? "Save Changes" : "Create Recipe"}
						</Button>
					)}
				</div>
			</div>
		</div>
	);
}

// ---------------------------------------------------------------------------
// Trigger recipe modal (device picker)
// ---------------------------------------------------------------------------

function TriggerModal({
	open,
	recipe,
	onClose,
}: {
	open: boolean;
	recipe: CommandRecipe | null;
	onClose: () => void;
}) {
	const [search, setSearch] = useState("");
	const [selected, setSelected] = useState<Set<number>>(new Set());
	const [done, setDone] = useState(false);

	const { data } = useGetDevices({
		search: search.trim() || undefined,
		limit: 50,
	});
	const devices = asArray<Device>(data);

	const triggerMut = useIssueCommandsToDevices({
		mutation: {
			onSuccess: () => setDone(true),
		},
	});

	const close = () => {
		if (triggerMut.isPending) return;
		setSearch("");
		setSelected(new Set());
		setDone(false);
		onClose();
	};

	const toggle = (id: number) =>
		setSelected((s) => {
			const next = new Set(s);
			if (next.has(id)) next.delete(id);
			else next.add(id);
			return next;
		});

	const handleTrigger = () => {
		if (!recipe || selected.size === 0) return;
		// A recipe is just a stored command list, so triggering it is the same as
		// issuing a normal command bundle with those commands.
		triggerMut.mutate({
			data: {
				devices: Array.from(selected),
				commands: recipe.commands,
			},
		});
	};

	return (
		<Modal
			open={open}
			onClose={close}
			title={`Trigger "${recipe?.name ?? ""}"`}
			subtitle="Creates a command bundle on the selected devices"
			footer={
				done ? (
					<Button variant="solid" tone="blue" onClick={close}>
						Done
					</Button>
				) : (
					<>
						<Button
							variant="soft"
							tone="gray"
							onClick={close}
							disabled={triggerMut.isPending}
						>
							Cancel
						</Button>
						<Button
							variant="solid"
							tone="purple"
							icon={<Send className="w-4 h-4" />}
							loading={triggerMut.isPending}
							disabled={selected.size === 0}
							onClick={handleTrigger}
						>
							Trigger on {selected.size}{" "}
							{selected.size === 1 ? "device" : "devices"}
						</Button>
					</>
				)
			}
		>
			{done ? (
				<div className="bg-green-50 border border-green-200 text-green-800 text-sm rounded-lg p-4">
					Recipe triggered on {selected.size}{" "}
					{selected.size === 1 ? "device" : "devices"}. The commands are queued
					and will run when each device checks in.
				</div>
			) : (
				<div className="space-y-3">
					<SearchInput
						value={search}
						onChange={setSearch}
						placeholder="Search devices by serial…"
					/>
					<div className="max-h-72 overflow-y-auto border border-gray-200/80 rounded-lg divide-y divide-gray-100">
						{devices.length === 0 ? (
							<p className="text-sm text-gray-400 italic p-3">
								No devices found.
							</p>
						) : (
							devices.map((d) => (
								<label
									key={d.id}
									className="flex items-center gap-3 px-3 py-2 cursor-pointer hover:bg-gray-50"
								>
									<input
										type="checkbox"
										checked={selected.has(d.id)}
										onChange={() => toggle(d.id)}
									/>
									<span className="text-sm font-mono text-gray-900">
										{d.serial_number}
									</span>
								</label>
							))
						)}
					</div>
				</div>
			)}
		</Modal>
	);
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

// "new" means the draft for an unsaved recipe is open in the editor.
type Selection = number | "new" | null;

export default function RecipesPage() {
	const queryClient = useQueryClient();
	const { data, isLoading } = useGetRecipes();
	const recipes = useMemo(() => asArray<CommandRecipe>(data), [data]);

	const [selected, setSelected] = useState<Selection>(null);
	const [triggering, setTriggering] = useState<CommandRecipe | null>(null);
	const [deleting, setDeleting] = useState<CommandRecipe | null>(null);
	const [pendingSelectName, setPendingSelectName] = useState<string | null>(
		null,
	);

	// Auto-select the first recipe once loaded (matches the other list pages).
	useEffect(() => {
		if (selected === null && recipes.length > 0) {
			setSelected(recipes[0].id);
		}
	}, [recipes, selected]);

	// After saving, select the (possibly newly created) recipe by name.
	useEffect(() => {
		if (pendingSelectName == null) return;
		const match = recipes.find((r) => r.name === pendingSelectName);
		if (match) {
			setSelected(match.id);
			setPendingSelectName(null);
		}
	}, [recipes, pendingSelectName]);

	const deleteMut = useDeleteRecipe({
		mutation: {
			onSuccess: () => {
				queryClient.invalidateQueries({ queryKey: getGetRecipesQueryKey() });
				setSelected(null);
				setDeleting(null);
			},
		},
	});

	const selectedRecipe =
		typeof selected === "number"
			? (recipes.find((r) => r.id === selected) ?? null)
			: null;
	const editorKey = selected === "new" ? "new" : (selected ?? "empty");

	return (
		<div className="flex-1 overflow-hidden p-4 sm:p-6 lg:p-8 flex flex-col">
			<Card className="flex-1 overflow-hidden flex">
				{/* Left: recipe list */}
				<div className="w-1/4 border-r border-gray-200/80 shrink-0 flex flex-col overflow-hidden">
					<div className="flex items-center justify-between px-4 py-3 border-b border-gray-200/80 shrink-0">
						<span className="text-sm font-semibold text-gray-900">Recipes</span>
						<button
							type="button"
							onClick={() => setSelected("new")}
							className="flex items-center gap-1 text-sm text-blue-600 hover:text-blue-800 cursor-pointer"
						>
							<Plus className="w-4 h-4" />
							New
						</button>
					</div>
					<div className="flex-1 overflow-y-auto overflow-x-hidden">
						{selected === "new" && (
							<button
								type="button"
								className="w-full text-left px-4 py-3 border-b border-gray-100 bg-blue-50 border-l-2 border-l-blue-500 cursor-default"
							>
								<span className="text-sm font-medium text-blue-900 italic">
									New Recipe…
								</span>
							</button>
						)}
						{isLoading ? (
							<p className="text-sm text-gray-400 px-4 py-3">Loading…</p>
						) : recipes.length === 0 && selected !== "new" ? (
							<div className="px-4 py-8 text-center">
								<ScrollText className="w-8 h-8 text-gray-300 mx-auto mb-2" />
								<p className="text-sm text-gray-500">No recipes yet</p>
								<p className="text-xs text-gray-400 mt-1">
									Click “New” to create one.
								</p>
							</div>
						) : (
							recipes.map((r) => {
								const isSelected = r.id === selected;
								const count = asArray<unknown>(r.commands).length;
								return (
									<button
										key={r.id}
										type="button"
										onClick={() => setSelected(r.id)}
										className={`w-full text-left px-4 py-3 border-b border-gray-100 last:border-b-0 transition-colors cursor-pointer ${
											isSelected
												? "bg-blue-50 border-l-2 border-l-blue-500"
												: "hover:bg-gray-50 border-l-2 border-l-transparent"
										}`}
									>
										<div className="flex items-center gap-2 mb-1 min-w-0">
											<ScrollText
												className={`w-3.5 h-3.5 shrink-0 ${isSelected ? "text-blue-500" : "text-purple-500"}`}
											/>
											<span
												className={`text-sm font-medium truncate min-w-0 ${isSelected ? "text-blue-900" : "text-gray-900"}`}
											>
												{r.name}
											</span>
										</div>
										<div className="flex items-center justify-between gap-2">
											<span className="text-xs text-gray-400">
												{count} {count === 1 ? "command" : "commands"}
											</span>
											<RelativeTime
												date={r.updated_at}
												className="text-xs text-gray-400 shrink-0"
											/>
										</div>
									</button>
								);
							})
						)}
					</div>
				</div>

				{/* Right: view / edit */}
				<div className="flex-1 overflow-hidden">
					{selected === null ? (
						<div className="flex items-center justify-center h-full text-gray-400 text-sm">
							Select a recipe to view or edit
						</div>
					) : (
						<RecipeEditor
							key={editorKey}
							recipe={selectedRecipe}
							onSaved={(savedName) => setPendingSelectName(savedName)}
							onDeleteRequest={() =>
								selectedRecipe && setDeleting(selectedRecipe)
							}
							onTrigger={() => selectedRecipe && setTriggering(selectedRecipe)}
						/>
					)}
				</div>
			</Card>

			<TriggerModal
				open={triggering != null}
				recipe={triggering}
				onClose={() => setTriggering(null)}
			/>
			<Modal
				open={deleting != null}
				onClose={() => setDeleting(null)}
				title="Delete Recipe"
				footer={
					<>
						<Button
							variant="soft"
							tone="gray"
							disabled={deleteMut.isPending}
							onClick={() => setDeleting(null)}
						>
							Cancel
						</Button>
						<Button
							variant="solid"
							tone="red"
							loading={deleteMut.isPending}
							onClick={() =>
								deleting && deleteMut.mutate({ recipeId: deleting.id })
							}
						>
							Delete
						</Button>
					</>
				}
			>
				<p className="text-sm text-gray-600">
					Delete recipe <span className="font-semibold">{deleting?.name}</span>?
					This cannot be undone. Already-triggered command bundles are not
					affected.
				</p>
			</Modal>
		</div>
	);
}
