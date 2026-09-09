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

// ---- GitHub Projects V2 ----

export interface ProjectV2Info {
	id: string;
	number: number;
	title: string;
	shortDescription: string;
	url: string;
	closed: boolean;
	updatedAt: string;
	ownerLogin: string;
	ownerType: "user" | "org";
}

export interface ProjectV2FieldOption {
	id: string;
	name: string;
}

export interface ProjectV2Field {
	id: string;
	name: string;
	dataType: string; // "SINGLE_SELECT" / "TEXT" / "NUMBER" / "DATE" / "ITERATION" ...
	options: ProjectV2FieldOption[]; // 仅单选字段（如 Status）有值
}

export interface ProjectV2Item {
	id: string;
	contentType: "Issue" | "PullRequest" | "DraftIssue" | "";
	title: string;
	number: number | null;
	state: string;
	url: string;
	repo: string; // nameWithOwner
	status: string | null; // Status 单选字段当前值
	statusOptionId: string | null;
	updatedAt: string;
}

export interface ProjectV2Board {
	project: ProjectV2Info;
	fields: ProjectV2Field[];
	items: ProjectV2Item[];
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
