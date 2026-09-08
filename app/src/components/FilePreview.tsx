import * as api from "../api";
import { TAB_LABELS } from "../navigation";
import { findCheckout, useStore } from "../store";
import MarkdownView from "./MarkdownView";
import { ResourceState, useResource } from "./ResourceState";

export default function FilePreview({
	fkey,
	co,
}: {
	fkey: string;
	co: string;
}) {
	const { projects, tab, returnToProject, toast } = useStore();
	const hit = findCheckout(projects, co);
	const rel = fkey.slice(co.length + 1);
	const name = rel.split("/").pop() ?? rel;
	const abs = hit ? hit.c.path + "/" + rel : null;
	// Markdown 里相对图片基于文件所在目录解析
	const baseDir = abs?.slice(0, abs.length - name.length).replace(/[\\/]$/, "");
	const preview = useResource(fkey, () =>
		abs ? api.readFilePreview(abs) : Promise.reject("工作树已不存在"),
	);
	return (
		<div className="content file-content">
			<div className="page-heading">
				<div>
					<button className="back-link" onClick={returnToProject}>
						← 返回{TAB_LABELS[tab] ?? "项目"}
					</button>
					<h2>{name}</h2>
					<p className="muted path-text">
						{hit?.c.branch} / {rel}
					</p>
				</div>
				<button
					className="btn"
					disabled={!abs}
					onClick={() =>
						abs && api.openInEditor(abs).catch((e) => toast(String(e)))
					}
				>
					在编辑器打开
				</button>
			</div>
			{preview.loading || preview.error ? (
				<ResourceState
					loading={preview.loading}
					error={preview.error}
					onRetry={preview.reload}
				/>
			) : preview.data?.isBinary ? (
				<ResourceState
					title="此文件无法直接预览"
					detail="二进制文件可在外部编辑器中打开。"
				/>
			) : preview.data && name.toLowerCase().endsWith(".md") ? (
				<MarkdownView text={preview.data.text} baseDir={baseDir ?? ""} />
			) : (
				<pre className="code">{preview.data?.text || "（空文件）"}</pre>
			)}
			{preview.data?.truncated && (
				<p className="muted">
					文件内容较长，当前只显示部分内容；完整文件可在编辑器查看。
				</p>
			)}
		</div>
	);
}
