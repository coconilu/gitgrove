import type { Update } from "@tauri-apps/plugin-updater";
import { createStore } from "zustand/vanilla";

export const DISMISSED_UPDATES_KEY = "gitgrove.updates.dismissed.v1";
type UpdateHandle = Pick<Update, "version" | "downloadAndInstall" | "close">;
type Dependencies = {
	getVersion: () => Promise<string>;
	check: () => Promise<UpdateHandle | null>;
	readDismissed: () => string | null;
	writeDismissed: (value: string) => void;
};
export type InstallStatus =
	| "idle"
	| "downloading"
	| "installing"
	| "installed"
	| "error";
export interface UpdateState {
	currentVersion: string | null;
	versionLoading: boolean;
	versionError: boolean;
	availableVersion: string | null;
	dismissedVersions: string[];
	checkStatus: "idle" | "checking" | "checked" | "error";
	installStatus: InstallStatus;
	downloaded: number;
	total: number | null;
}

export function installLocked(status: InstallStatus) {
	return (
		status === "downloading" ||
		status === "installing" ||
		status === "installed"
	);
}

// The updater handle belongs to the app, never to a toast or an open dialog.
export function createUpdateController(deps: Dependencies) {
	let candidate: UpdateHandle | null = null;
	let dismissedVersions: string[] = [];
	try {
		const saved: unknown = JSON.parse(deps.readDismissed() ?? "[]");
		if (Array.isArray(saved))
			dismissedVersions = saved.filter(
				(v): v is string => typeof v === "string",
			);
	} catch {
		// Storage may be unavailable; dismissal still works for this session.
	}
	const store = createStore<UpdateState>(() => ({
		currentVersion: null,
		versionLoading: false,
		versionError: false,
		availableVersion: null,
		dismissedVersions,
		checkStatus: "idle",
		installStatus: "idle",
		downloaded: 0,
		total: null,
	}));
	const release = async (handle: UpdateHandle | null) => {
		try {
			await handle?.close();
		} catch {
			// A failed cleanup must not discard the newly checked update.
		}
	};

	const loadVersion = async () => {
		if (store.getState().versionLoading || store.getState().currentVersion)
			return;
		store.setState({ versionLoading: true, versionError: false });
		try {
			store.setState({ currentVersion: await deps.getVersion() });
		} catch {
			store.setState({ versionError: true });
		} finally {
			store.setState({ versionLoading: false });
		}
	};

	const checkForUpdates = async () => {
		const state = store.getState();
		if (state.checkStatus === "checking" || installLocked(state.installStatus))
			return;
		store.setState({ checkStatus: "checking" });
		try {
			const next = await deps.check();
			const previous = candidate;
			candidate = next;
			store.setState({
				availableVersion: next?.version ?? null,
				checkStatus: "checked",
				installStatus: "idle",
				downloaded: 0,
				total: null,
			});
			if (previous !== next) await release(previous);
		} catch {
			// Keep an already discovered update usable when a later check fails.
			store.setState({ checkStatus: "error" });
		}
	};

	const dismiss = () => {
		const { availableVersion, dismissedVersions } = store.getState();
		if (!availableVersion || dismissedVersions.includes(availableVersion))
			return;
		const next = [...dismissedVersions, availableVersion];
		store.setState({ dismissedVersions: next });
		try {
			deps.writeDismissed(JSON.stringify(next));
		} catch {
			// Retain the in-memory dismissal even if persistence is unavailable.
		}
	};

	const install = async () => {
		const state = store.getState();
		if (
			!candidate ||
			state.checkStatus === "checking" ||
			installLocked(state.installStatus)
		)
			return;
		const selected = candidate;
		// Set the shared lock synchronously, before the first asynchronous operation.
		store.setState({
			installStatus: "downloading",
			downloaded: 0,
			total: null,
		});
		try {
			await selected.downloadAndInstall(
				(event) => {
					if (event.event === "Started") {
						store.setState({
							total: event.data.contentLength ?? null,
							downloaded: 0,
						});
					} else if (event.event === "Progress") {
						store.setState((s) => ({
							downloaded: s.downloaded + event.data.chunkLength,
						}));
					} else {
						store.setState({ installStatus: "installing" });
					}
				},
				{ timeout: 120_000 },
			);
			// Windows exits into the installer. Other platforms need a manual restart.
			store.setState({ installStatus: "installed" });
		} catch {
			store.setState({ installStatus: "error" });
		}
	};

	return { store, loadVersion, checkForUpdates, dismiss, install };
}
