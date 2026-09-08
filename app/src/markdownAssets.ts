// Markdown 里的本地资源路径：把相对图片 src 解析为基于 Markdown 文件目录的绝对路径，
// 供 asset protocol（convertFileSrc）加载。GitHub 在线上渲染时做同样的重写。

// 带 scheme（https:、data:、asset: 等）、协议相对（//host/x）和页内锚点（#x）都不是本地路径
const REMOTE_RE = /^([a-z][a-z0-9+.-]*:|\/\/|#)/i;

export function isLocalImageSrc(src: string): boolean {
	return !REMOTE_RE.test(src);
}

export function resolveLocalPath(baseDir: string, src: string): string {
	const queryless = src.split(/[?#]/, 1)[0];
	let decoded = queryless;
	try {
		decoded = decodeURIComponent(queryless);
	} catch {
		// 含非法 % 转义时按原样处理
	}
	const sep = baseDir.includes("\\") ? "\\" : "/";
	const root = baseDir.startsWith("\\\\")
		? "\\\\"
		: baseDir.startsWith("/")
			? "/"
			: "";
	const parts = baseDir.split(/[\\/]+/).filter((s) => s.length > 0);
	for (const seg of decoded.replace(/^\/+/, "").split("/")) {
		if (seg === "" || seg === ".") continue;
		if (seg === "..") {
			if (parts.length > 1) parts.pop();
			continue;
		}
		parts.push(seg);
	}
	return root + parts.join(sep);
}

// 在 HTML 字符串层面改写 <img> 的相对 src（React 19 重渲染会按引用比较
// dangerouslySetInnerHTML 并重置 innerHTML，事后改 DOM 会被覆盖）
const IMG_TAG_RE = /<img\b[^>]*?>/gi;
const SRC_ATTR_RE = /\bsrc\s*=\s*("([^"]*)"|'([^']*)'|([^\s>]+))/i;

export function rewriteLocalImages(
	html: string,
	baseDir: string,
	toUrl: (absPath: string) => string,
): string {
	return html.replace(IMG_TAG_RE, (tag) => {
		const m = SRC_ATTR_RE.exec(tag);
		if (!m) return tag;
		const src = m[2] ?? m[3] ?? m[4] ?? "";
		if (!isLocalImageSrc(src)) return tag;
		const url = toUrl(resolveLocalPath(baseDir, src));
		const quote = m[1].startsWith("'") ? "'" : '"';
		return tag.replace(SRC_ATTR_RE, `src=${quote}${url}${quote}`);
	});
}
