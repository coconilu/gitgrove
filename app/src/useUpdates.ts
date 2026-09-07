import { getVersion } from "@tauri-apps/api/app";
import { check } from "@tauri-apps/plugin-updater";
import { useStore } from "zustand";
import { createUpdateController, DISMISSED_UPDATES_KEY } from "./updates";

export const updates = createUpdateController({
	getVersion,
	check: () => check({ timeout: 15_000 }),
	readDismissed: () => localStorage.getItem(DISMISSED_UPDATES_KEY),
	writeDismissed: (value) => localStorage.setItem(DISMISSED_UPDATES_KEY, value),
});

export const useUpdates = () => useStore(updates.store);
