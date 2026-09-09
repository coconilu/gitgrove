// GitHub Projects V2 只读面板的类型与命令封装。
// 临时放置：后端命令由 M1（app/src-tauri/**、api.ts、types.ts）实现，
// M1 合并后本文件的类型应迁入 types.ts、封装应迁入 api.ts，组件改从那里导入。
import { invoke } from "@tauri-apps/api/core";

export interface ProjectV2Info {
	id: string;
	number: number;
	title: string;
	url: string;
	closed: boolean;
	updatedAt: string;
}

export interface ProjectV2FieldOption {
	id: string;
	name: string;
}

export interface ProjectV2Field {
	id: string;
	name: string;
	dataType: string; // TEXT / NUMBER / DATE / SINGLE_SELECT / ITERATION / ...
	options: ProjectV2FieldOption[]; // 仅 SINGLE_SELECT 有值
}

export interface ProjectV2Item {
	id: string;
	kind: "issue" | "pr" | "draft";
	title: string;
	url: string | null; // draft 无 url
	repo: string | null; // "owner/repo"，draft 为 null
	number: number | null; // draft 为 null
	state: string | null; // open / closed / merged
	fieldValues: Record<string, string | null>; // 键为字段名，值已格式化为展示文本
}

export interface ProjectV2Details {
	fields: ProjectV2Field[];
	items: ProjectV2Item[];
}

export const listOwnerProjects = (owner: string) =>
	invoke<ProjectV2Info[]>("list_owner_projects", { owner });
export const projectV2Details = (owner: string, projectNumber: number) =>
	invoke<ProjectV2Details>("project_v2_details", { owner, projectNumber });

// 后端在 token 缺少 project scope 时以此标记拒绝，前端据此展示重新授权引导。
export const isMissingProjectScope = (error: string) =>
	/MissingProjectScope/i.test(error);
