import { createContext, type ReactNode, useContext } from "react";

const CommandPaletteContext = createContext<(() => void) | null>(null);

export function CommandPaletteProvider({
	open,
	children,
}: {
	open: () => void;
	children: ReactNode;
}) {
	return (
		<CommandPaletteContext.Provider value={open}>
			{children}
		</CommandPaletteContext.Provider>
	);
}

/** Opens the global command palette (⌘K search). Must be rendered under `PrivateLayout`. */
export function useCommandPalette() {
	const open = useContext(CommandPaletteContext);
	if (!open) {
		throw new Error(
			"useCommandPalette must be used within a CommandPaletteProvider",
		);
	}
	return open;
}
