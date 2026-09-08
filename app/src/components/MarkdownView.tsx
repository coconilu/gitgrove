import { convertFileSrc } from "@tauri-apps/api/core";
import { marked } from "marked";
import { useEffect, useMemo, useState } from "react";
import * as api from "../api";
import { rewriteLocalImages } from "../markdownAssets";

// 已授权的目录缓存：asset protocol 作用域是进程级的，同一目录只需授权一次
const allowedDirs = new Set<string>();

// 本地 Markdown 渲染：相对路径图片在 HTML 字符串层面解析到 Markdown 所在目录，
// 经 asset protocol 加载；授权完成前不出 HTML，避免浏览器在作用域生效前拉取图片
export default function MarkdownView({
	text,
	baseDir,
}: {
	text: string;
	baseDir: string;
}) {
	const [ready, setReady] = useState(!baseDir || allowedDirs.has(baseDir));
	useEffect(() => {
		if (!baseDir || allowedDirs.has(baseDir)) return;
		let cancelled = false;
		api
			.allowAssetScope(baseDir)
			.then(() => {
				allowedDirs.add(baseDir);
				if (!cancelled) setReady(true);
			})
			.catch(() => {});
		return () => {
			cancelled = true;
		};
	}, [baseDir]);
	const html = useMemo(() => {
		if (!ready) return null;
		const parsed = marked.parse(text) as string;
		return baseDir
			? rewriteLocalImages(parsed, baseDir, convertFileSrc)
			: parsed;
	}, [text, baseDir, ready]);
	if (html === null) return <div className="card md" />;
	return (
		<div
			className="card md"
			// biome-ignore lint/security/noDangerouslySetInnerHtml: 现有本地 Markdown 预览行为
			dangerouslySetInnerHTML={{ __html: html }}
		/>
	);
}
