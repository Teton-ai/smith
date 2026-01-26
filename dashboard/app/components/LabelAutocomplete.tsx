"use client";

import { useEffect, useRef, useState } from "react";
import { type LabelWithValues, useGetLabels } from "../api-client";

interface LabelAutocompleteProps {
	onSelect: (label: string) => void;
	existingFilters: string[];
}

type AutocompleteState =
	| { mode: "key"; search: string }
	| { mode: "value"; key: string; search: string };

export default function LabelAutocomplete({
	onSelect,
	existingFilters,
}: LabelAutocompleteProps) {
	const [isOpen, setIsOpen] = useState(false);
	const [state, setState] = useState<AutocompleteState>({
		mode: "key",
		search: "",
	});
	const inputRef = useRef<HTMLInputElement>(null);
	const dropdownRef = useRef<HTMLDivElement>(null);

	const { data: labels } = useGetLabels();

	useEffect(() => {
		const handleClickOutside = (event: MouseEvent) => {
			if (
				dropdownRef.current &&
				!dropdownRef.current.contains(event.target as Node) &&
				inputRef.current &&
				!inputRef.current.contains(event.target as Node)
			) {
				setIsOpen(false);
			}
		};

		document.addEventListener("mousedown", handleClickOutside);
		return () => document.removeEventListener("mousedown", handleClickOutside);
	}, []);

	const getFilteredKeys = (): LabelWithValues[] => {
		if (!labels) return [];
		const search = state.search.toLowerCase();
		return labels.filter((label) => label.key.toLowerCase().includes(search));
	};

	const getFilteredValues = (key: string): string[] => {
		if (!labels) return [];
		const label = labels.find((l) => l.key === key);
		if (!label) return [];

		if (state.mode !== "value") return label.values;

		const search = state.search.toLowerCase();
		return label.values.filter((value) => value.toLowerCase().includes(search));
	};

	const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
		const value = e.target.value;

		if (state.mode === "key") {
			if (value.includes("=")) {
				const [key, rest] = value.split("=");
				const matchingLabel = labels?.find(
					(l) => l.key.toLowerCase() === key.toLowerCase(),
				);
				if (matchingLabel) {
					setState({ mode: "value", key: matchingLabel.key, search: rest });
				} else {
					setState({ mode: "key", search: value });
				}
			} else {
				setState({ mode: "key", search: value });
			}
		} else {
			if (!value.includes("=")) {
				setState({ mode: "key", search: value });
			} else {
				const [, rest] = value.split("=");
				setState({ ...state, search: rest });
			}
		}

		setIsOpen(true);
	};

	const handleKeySelect = (key: string) => {
		setState({ mode: "value", key, search: "" });
		if (inputRef.current) {
			inputRef.current.value = `${key}=`;
			inputRef.current.focus();
		}
	};

	const handleValueSelect = (value: string) => {
		if (state.mode === "value") {
			const filter = `${state.key}=${value}`;
			if (!existingFilters.includes(filter)) {
				onSelect(filter);
			}
			setState({ mode: "key", search: "" });
			if (inputRef.current) {
				inputRef.current.value = "";
			}
			setIsOpen(false);
		}
	};

	const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
		if (e.key === "Enter") {
			const value = e.currentTarget.value;
			if (value.includes("=")) {
				const [key, val] = value.split("=");
				if (key && val) {
					const filter = `${key.trim()}=${val.trim()}`;
					if (!existingFilters.includes(filter)) {
						onSelect(filter);
					}
					setState({ mode: "key", search: "" });
					e.currentTarget.value = "";
					setIsOpen(false);
				}
			}
		} else if (e.key === "Escape") {
			setIsOpen(false);
		} else if (e.key === "Backspace" && state.mode === "value") {
			if (state.search === "") {
				setState({ mode: "key", search: state.key });
				if (inputRef.current) {
					inputRef.current.value = state.key;
				}
			}
		}
	};

	const getInputValue = () => {
		if (state.mode === "value") {
			return `${state.key}=${state.search}`;
		}
		return state.search;
	};

	const filteredKeys = getFilteredKeys();
	const filteredValues =
		state.mode === "value" ? getFilteredValues(state.key) : [];

	return (
		<div className="relative">
			<input
				ref={inputRef}
				type="text"
				placeholder="Filter by label (e.g., env=prod)"
				className="w-64 px-3 py-2 text-sm border border-gray-300 rounded-md focus:ring-2 focus:ring-blue-500 focus:border-blue-500 text-gray-900 placeholder-gray-400"
				value={getInputValue()}
				onChange={handleInputChange}
				onFocus={() => setIsOpen(true)}
				onKeyDown={handleKeyDown}
			/>

			{isOpen && labels && labels.length > 0 && (
				<div
					ref={dropdownRef}
					className="absolute z-50 mt-1 w-64 bg-white border border-gray-200 rounded-md shadow-lg max-h-60 overflow-auto"
				>
					{state.mode === "key" ? (
						filteredKeys.length > 0 ? (
							<>
								<div className="px-3 py-1.5 text-xs text-gray-500 bg-gray-50 border-b border-gray-100">
									Select a label key
								</div>
								{filteredKeys.map((label) => (
									<button
										key={label.key}
										type="button"
										className="w-full px-3 py-2 text-left text-sm text-gray-700 hover:bg-blue-50 hover:text-blue-700 cursor-pointer"
										onClick={() => handleKeySelect(label.key)}
									>
										<span className="font-medium">{label.key}</span>
										<span className="text-gray-400 ml-2">
											({label.values.length} values)
										</span>
									</button>
								))}
							</>
						) : (
							<div className="px-3 py-2 text-sm text-gray-500">
								No matching keys
							</div>
						)
					) : (
						<>
							<div className="px-3 py-1.5 text-xs text-gray-500 bg-gray-50 border-b border-gray-100 flex items-center justify-between">
								<span>
									Values for <span className="font-medium">{state.key}</span>
								</span>
								<button
									type="button"
									className="text-blue-600 hover:text-blue-800"
									onClick={() => {
										setState({ mode: "key", search: "" });
										if (inputRef.current) {
											inputRef.current.value = "";
											inputRef.current.focus();
										}
									}}
								>
									Back
								</button>
							</div>
							{filteredValues.length > 0 ? (
								filteredValues.map((value) => {
									const isAlreadySelected = existingFilters.includes(
										`${state.key}=${value}`,
									);
									return (
										<button
											key={value}
											type="button"
											disabled={isAlreadySelected}
											className={`w-full px-3 py-2 text-left text-sm cursor-pointer ${
												isAlreadySelected
													? "text-gray-400 bg-gray-50"
													: "text-gray-700 hover:bg-blue-50 hover:text-blue-700"
											}`}
											onClick={() => handleValueSelect(value)}
										>
											{value}
											{isAlreadySelected && (
												<span className="ml-2 text-xs">(already added)</span>
											)}
										</button>
									);
								})
							) : (
								<div className="px-3 py-2 text-sm text-gray-500">
									No matching values
								</div>
							)}
						</>
					)}
				</div>
			)}
		</div>
	);
}
