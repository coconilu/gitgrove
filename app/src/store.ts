import { create } from "zustand";
import * as api from "./api";
import {
	ciStatus,
	navigate,
	previewParent,
	projectVisit,
	selectionProject,
	type Visit,
} from "./navigation";
import type {
	AuthState,
	CheckoutInfo,
	CiStatus,
	Project,
	RepoInfo,
	Selection,
	StatusMap,
} from "./types";

export interface DialogField {
	name: string;
	label: string;
	type?: "text" | "select" | "textarea" | "number";
	defaultValue?: string;
	required?: boolean;
	options?: string[];
	help?: string;
}
export interface DialogState {
	kind: "prompt" | "confirm";
	title: string;
	message?: string;
	placeholder?: string;
	defaultValue?: string;
	okText?: string;
	danger?: boolean;
	secondaryText?: string;
	fields?: DialogField[];
	describe?: (value: string, values: Record<string, string>) => string;
	validate?: (value: string, values: Record<string, string>) => string | null;
	onSubmit: (
		value: string,
		values: Record<string, string>,
	) => void | Promise<void>;
	onSecondary?: () => void | Promise<void>;
}
type Layout = {
	sidebarOpen: boolean;
	filesOpen: boolean;
	sidebarWidth: number;
	filesWidth: number;
};
const defaultLayout: Layout = {
	sidebarOpen: true,
	filesOpen: false,
	sidebarWidth: 248,
	filesWidth: 280,
};
function readLayout(): Layout {
	try {
		const value = JSON.parse(
			localStorage.getItem("gitgrove.layout.v1") ?? "{}",
		);
		return {
			sidebarOpen:
				typeof value.sidebarOpen === "boolean" ? value.sidebarOpen : true,
			filesOpen: typeof value.filesOpen === "boolean" ? value.filesOpen : false,
			sidebarWidth: Number.isFinite(value.sidebarWidth)
				? Math.min(340, Math.max(200, value.sidebarWidth))
				: 248,
			filesWidth: Number.isFinite(value.filesWidth)
				? Math.min(420, Math.max(240, value.filesWidth))
				: 280,
		};
	} catch {
		return defaultLayout;
	}
}
interface AppState {
	auth: AuthState | null;
	authLoaded: boolean;
	view: "projects" | "github";
	sel: Selection | null;
	tab: string;
	visits: Record<string, Visit>;
	layout: Layout;
	groupBy: "repo" | "status";
	projects: Project[];
	expanded: Record<string, boolean>;
	projectsLoading: boolean;
	projectsError: string;
	myRepos: RepoInfo[] | null;
	reposLoading: boolean;
	reposError: string;
	ci: Record<string, CiStatus>;
	statusMaps: Record<string, StatusMap>;
	statusErrors: Record<string, string>;
	cloneProgress: string | null;
	toastMsg: string;
	dialog: DialogState | null;
	workItemFocus: { pid: string; number: number; type: "issue" | "pr" } | null;
	init: () => Promise<void>;
	reloadAuth: () => Promise<void>;
	refreshProjects: (keepSel?: boolean) => Promise<void>;
	refreshMyRepos: () => Promise<void>;
	refreshCi: (p: Project) => Promise<void>;
	refreshStatusMap: (c: CheckoutInfo) => Promise<void>;
	setView: (v: "projects" | "github") => void;
	setSel: (s: Selection | null) => void;
	openProject: (pid: string) => void;
	returnToProject: () => void;
	setTab: (tab: string) => void;
	setLayout: (layout: Partial<Layout>) => void;
	setGroupBy: (g: "repo" | "status") => void;
	setWorkItemFocus: (f: AppState["workItemFocus"]) => void;
	toggleProject: (pid: string) => void;
	setCloneProgress: (s: string | null) => void;
	toast: (msg: string) => void;
	openDialog: (dialog: DialogState) => void;
	closeDialog: () => void;
}
let toastTimer: ReturnType<typeof setTimeout> | undefined;
let projectsRequest = 0;
let reposRequest = 0;
export const useStore = create<AppState>((set, get) => ({
	auth: null,
	authLoaded: false,
	view: "projects",
	sel: null,
	tab: "Overview",
	visits: {},
	layout: readLayout(),
	groupBy: "repo",
	projects: [],
	expanded: {},
	myRepos: null,
	projectsLoading: false,
	projectsError: "",
	reposLoading: false,
	reposError: "",
	ci: {},
	statusMaps: {},
	statusErrors: {},
	cloneProgress: null,
	toastMsg: "",
	dialog: null,
	workItemFocus: null,
	init: async () => get().reloadAuth(),
	reloadAuth: async () => {
		try {
			const auth = await api.authStatus();
			set({ auth, authLoaded: true });
			if (auth.loggedIn)
				await Promise.all([get().refreshProjects(), get().refreshMyRepos()]);
			else {
				projectsRequest++;
				reposRequest++;
				set({
					projects: [],
					myRepos: null,
					sel: null,
					visits: {},
					ci: {},
					statusMaps: {},
					dialog: null,
				});
			}
		} catch {
			set({ auth: null, authLoaded: true });
		}
	},
	refreshProjects: async (keepSel = true) => {
		const request = ++projectsRequest;
		set({ projectsLoading: true, projectsError: "" });
		try {
			const projects = await api.listProjects();
			if (request !== projectsRequest) return;
			const expanded = { ...get().expanded };
			for (const p of projects) if (!(p.id in expanded)) expanded[p.id] = true;
			set({ projects, expanded });
			const sel = get().sel;
			if (!keepSel || !selectionProject(projects, sel))
				get().setSel(
					projects.length ? { kind: "project", pid: projects[0].id } : null,
				);
		} catch (error) {
			if (request === projectsRequest) set({ projectsError: String(error) });
		} finally {
			if (request === projectsRequest) set({ projectsLoading: false });
		}
	},
	refreshMyRepos: async () => {
		const request = ++reposRequest;
		set({ reposLoading: true, reposError: "" });
		try {
			const myRepos = await api.listMyRepos();
			if (request === reposRequest) set({ myRepos });
		} catch (error) {
			if (request === reposRequest) set({ reposError: String(error) });
		} finally {
			if (request === reposRequest) set({ reposLoading: false });
		}
	},
	refreshCi: async (p) => {
		const gh = p.providerIdentity;
		if (!gh) return;
		// 小批并行，避免串行瀑布，也不同时请求所有仓库。
		for (let i = 0; i < p.checkouts.length; i += 4)
			await Promise.all(
				p.checkouts.slice(i, i + 4).map(async (co) => {
					try {
						const run = await api.latestRunForBranch(
							gh.owner,
							gh.repo,
							co.branch,
						);
						set((s) => ({ ci: { ...s.ci, [co.id]: ciStatus(run) } }));
					} catch {
						set((s) => ({ ci: { ...s.ci, [co.id]: "unknown" } }));
					}
				}),
			);
	},
	refreshStatusMap: async (co) => {
		try {
			const map = await api.checkoutStatus(co.path);
			set((s) => ({
				statusMaps: { ...s.statusMaps, [co.id]: map },
				statusErrors: { ...s.statusErrors, [co.id]: "" },
			}));
		} catch (error) {
			set((s) => ({
				statusErrors: { ...s.statusErrors, [co.id]: String(error) },
			}));
		}
	},
	setView: (view) => set({ view }),
	setSel: (sel) =>
		set((s) => ({ ...navigate(s.projects, s, sel), workItemFocus: null })),
	openProject: (pid) =>
		set((s) => {
			const current = selectionProject(s.projects, s.sel);
			const visit =
				current?.id === pid && s.sel
					? { sel: s.sel, tab: s.tab }
					: projectVisit(s.projects, s.visits, pid);
			return {
				...navigate(s.projects, s, visit.sel),
				tab: visit.tab,
				view: "projects",
				workItemFocus: null,
			};
		}),
	returnToProject: () => get().setSel(previewParent(get().sel)),
	setTab: (tab) =>
		set((s) => {
			const sel = previewParent(s.sel);
			const next = navigate(s.projects, s, sel);
			const p = selectionProject(s.projects, sel);
			return {
				...next,
				tab,
				visits:
					p && sel ? { ...next.visits, [p.id]: { sel, tab } } : next.visits,
				workItemFocus: null,
			};
		}),
	setWorkItemFocus: (workItemFocus) => set({ workItemFocus }),
	setLayout: (patch) =>
		set((s) => {
			const layout = { ...s.layout, ...patch };
			try {
				localStorage.setItem("gitgrove.layout.v1", JSON.stringify(layout));
			} catch {
				/* 会话内仍可使用 */
			}
			return { layout };
		}),
	setGroupBy: (groupBy) => set({ groupBy }),
	toggleProject: (pid) =>
		set((s) => ({
			expanded: { ...s.expanded, [pid]: !(s.expanded[pid] ?? true) },
		})),
	setCloneProgress: (cloneProgress) => set({ cloneProgress }),
	toast: (toastMsg) => {
		set({ toastMsg });
		if (toastTimer) clearTimeout(toastTimer);
		toastTimer = setTimeout(() => set({ toastMsg: "" }), 5000);
	},
	openDialog: (dialog) => set({ dialog }),
	closeDialog: () => set({ dialog: null }),
}));

export function findCheckout(
	projects: Project[],
	cid: string,
): { p: Project; c: CheckoutInfo } | null {
	for (const p of projects)
		for (const c of p.checkouts) if (c.id === cid) return { p, c };
	return null;
}
export function activeCheckout(
	projects: Project[],
	sel: Selection | null,
): { p: Project; c: CheckoutInfo } | null {
	if (!sel) return null;
	if (sel.kind === "checkout") return findCheckout(projects, sel.cid);
	if (sel.kind === "file") return findCheckout(projects, sel.co);
	const p = projects.find((x) => x.id === sel.pid);
	const c = p?.checkouts.find((x) => x.isPrimary);
	return p && c ? { p, c } : null;
}
