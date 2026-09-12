// 与 Rust 后端共享的数据结构（serde camelCase）

export interface LinkedWorkItem {
	type: "issue" | "pr";
	number: number;
	title: string;
	url: string;
}

export interface CheckoutInfo {
	id: string; // `${projectId}::${path}`
	path: string;
	branch: string;
	isPrimary: boolean;
	isLocked: boolean;
	linkedWorkItem: LinkedWorkItem | null;
	ahead: number;
	behind: number;
}

export interface ProviderIdentity {
	provider: string; // "github"，预留 "gitlab" 等
	owner: string;
	repo: string;
}

export interface Project {
	id: string;
	name: string; // 展示名：GitHub 仓库名或目录名
	providerIdentity: ProviderIdentity | null; // 本地/非 GitHub 仓库为 null
	isPrivate: boolean;
	forkOf: string | null;
	color: string;
	localPath: string;
	defaultBranch: string;
	addedAt: number;
	checkouts: CheckoutInfo[];
}

export interface AuthState {
	loggedIn: boolean;
	login: string;
	name: string;
	avatarUrl: string;
	source: string; // "keyring" | "gh-cli" | "pat"
	/** 当前 token 是否具备 project scope；null = 无法判断（如 fine-grained PAT 无 X-OAuth-Scopes 头） */
	hasProjectScope: boolean | null;
}

// ---- 自研项目管理（PM） ----

/** 看板列定义，持久化在后端 settings 表 */
export interface PmStatusDef {
	id: string;
	name: string;
}

export type PmPriority = "none" | "low" | "medium" | "high" | "urgent";

export interface PmItem {
	id: string;
	title: string;
	body: string;
	status: string;
	priority: PmPriority | string;
	milestoneId: string | null;
	labels: string[];
	repoPath: string | null;
	branch: string | null;
	/** YYYY-MM-DD */
	dueDate: string | null;
	/** 看板列内排序键（fractional indexing，字符串序） */
	order: string;
	/** P2 GitHub 互通引用，格式 `owner/repo#number`；同步引擎按此匹配 upsert */
	githubRef: string | null;
	/** 人工拖动过的 GitHub 同步卡片：同步时字段仍刷新，但列位置不再自动迁移 */
	manualLock: boolean;
	/** 进入最后一列（done）的时间戳（unix 秒）；离开最后一列或老数据为 null */
	closedAt?: number | null;
	createdAt: number;
	updatedAt: number;
}

export interface PmMilestone {
	id: string;
	title: string;
	description: string;
	dueDate: string | null;
	status: string; // open | closed
	githubRef: string | null;
	createdAt: number;
	updatedAt: number;
}

/** pm_list_milestones 返回：附带 item 聚合（done = 最后一个看板列） */
export interface PmMilestoneWithStats extends PmMilestone {
	total: number;
	done: number;
}

/** pm_list_items 过滤参数；labels 为「同时包含」语义 */
export interface PmItemFilter {
	milestoneId?: string | null;
	repoPath?: string | null;
	status?: string | null;
	labels?: string[] | null;
	priority?: string | null;
	search?: string | null;
}

/** pm_create_item 入参（id / order / 时间戳由后端生成） */
export interface PmNewItem {
	title: string;
	body?: string;
	status?: string | null;
	priority?: string | null;
	milestoneId?: string | null;
	labels?: string[];
	repoPath?: string | null;
	branch?: string | null;
	dueDate?: string | null;
}

/** pm_export_json / pm_import_json 的整库快照 */
export interface PmExport {
	version: number;
	statuses: PmStatusDef[];
	items: PmItem[];
	milestones: PmMilestone[];
}

/** pm_import_json 返回的计数，供前端 toast */
export interface PmImportResult {
	items: number;
	milestones: number;
}

/** pm_sync_github 返回的同步统计：新建/更新/自动迁移的卡片数 */
export interface PmSyncResult {
	created: number;
	updated: number;
	moved: number;
}

export interface RepoInfo {
	name: string;
	nameWithOwner: string;
	isPrivate: boolean;
	isFork: boolean;
	parent: string | null;
	language: string | null;
	description: string | null;
	pushedAt: string | null;
	url: string;
}

export interface LabelInfo {
	name: string;
	color: string;
}

export interface IssueInfo {
	number: number;
	title: string;
	state: string;
	labels: LabelInfo[];
	assignee: string | null;
	url: string;
	createdAt: string;
}

export interface PrInfo {
	number: number;
	title: string;
	state: string; // open / closed
	merged: boolean;
	draft: boolean;
	user: string;
	headRef: string;
	url: string;
	createdAt: string;
}

export interface BranchInfo {
	name: string;
	remote: boolean;
	ahead: number;
	behind: number;
	lastCommitTs: number; // unix seconds
	sha: string;
	subject: string;
}

export interface WorkflowInfo {
	id: number;
	name: string;
	path: string;
	state: string;
}

export interface WorkflowInput {
	name: string;
	description: string;
	kind: string;
	required: boolean;
	defaultValue: string;
	options: string[];
}
export interface WorkflowDetails {
	defaultBranch: string;
	dispatch: boolean;
	inputs: WorkflowInput[];
}

export interface RunInfo {
	id: number;
	runNumber: number;
	name: string;
	branch: string;
	status: string; // queued / in_progress / completed
	conclusion: string | null; // success / failure / ...
	actor: string;
	createdAt: string;
	url: string;
}

export interface DirEntry {
	name: string;
	isDir: boolean;
}

export interface StatusMap {
	changes: { path: string; code: string }[]; // code: M / A / D
	ignored: string[]; // 相对路径（不含尾斜杠）
}

export interface FilePreview {
	text: string;
	truncated: boolean;
	isBinary: boolean;
}

export interface CloneTargetInfo {
	target: string;
	exists: boolean;
}

export type CiStatus =
	| "ok"
	| "run"
	| "fail"
	| "none"
	| "unknown"
	| "cancelled"
	| "skipped"
	| null;

export type Selection =
	| { kind: "project"; pid: string }
	| { kind: "checkout"; cid: string }
	| { kind: "file"; key: string; co: string };
export interface LaunchPreferences {
	editor: string;
	terminal: string;
}
export interface LaunchApp {
	id: string;
	name: string;
	kind: "editor" | "terminal";
	path: string | null;
	running: boolean | null;
}
