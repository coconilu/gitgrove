import { convertFileSrc } from "@tauri-apps/api/core";
import { marked } from "marked";
import { useEffect, useMemo, useRef } from "react";
import * as api from "../api";
import { isLocalImageSrc, resolveLocalPath } from "../markdownAssets";

// 本地 Markdown 渲染：相对路径图片解析到 Markdown 所在目录，经 asset protocol 加载
export default function MarkdownView({
	text,
	baseDir,
}: {
	text: string;
	baseDir: string;
}) {
	const ref = useRef<HTMLDivElement>(null);
	const html = useMemo(() => marked.parse(text) as string, [text]);
	// biome-ignore lint/correctness/useExhaustiveDependencies: html 变化意味着 innerHTML 被整体替换，必须重新改写其中的图片 src
	useEffect(() => {
		const root = ref.current;
		if (!root) return;
		const imgs = Array.from(root.querySelectorAll("img")).filter((img) => {
			const src = img.getAttribute("src");
			return src !== null && isLocalImageSrc(src);
		});
		if (imgs.length === 0) return;
		let cancelled = false;
		api
			.allowAssetScope(baseDir)
			.then(() => {
				if (cancelled) return;
				for (const img of imgs) {
					const src = img.getAttribute("src");
					if (src) img.src = convertFileSrc(resolveLocalPath(baseDir, src));
				}
			})
			.catch(() => {});
		return () => {
			cancelled = true;
		};
	}, [html, baseDir]);
	return (
		<div
			ref={ref}
			className="card md"
			// biome-ignore lint/security/noDangerouslySetInnerHtml: 现有本地 Markdown 预览行为
			dangerouslySetInnerHTML={{ __html: html }}
		/>
	);
}
