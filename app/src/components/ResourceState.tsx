import { useEffect, useRef, useState } from "react";
import { peekSwr, type SwrState, swrFetch } from "../store";

function initialSwr<T>(key: string, cache: boolean): SwrState<T> {
	const cached = cache ? peekSwr<T>(key) : undefined;
	return cached !== undefined
		? { data: cached, error: "", loading: false, refreshing: true }
		: { data: null, error: "", loading: true, refreshing: false };
}

export function useResource<T>(
	key: string,
	load: () => Promise<T>,
	opts?: { cache?: boolean },
) {
	const useCache = opts?.cache === true;
	const loader = useRef(load);
	loader.current = load;
	const [revision, setRevision] = useState(0);
	const [state, setState] = useState<SwrState<T> & { key: string }>(() => ({
		key,
		...initialSwr<T>(key, useCache),
	}));
	// biome-ignore lint/correctness/useExhaustiveDependencies: revision 是显式重试信号
	useEffect(() => {
		return swrFetch<T>({
			key,
			cache: useCache,
			load: () => loader.current(),
			emit: (next) => setState({ key, ...next }),
		});
	}, [key, revision, useCache]);
	return {
		...(state.key === key ? state : { key, ...initialSwr<T>(key, useCache) }),
		reload: () => setRevision((n) => n + 1),
	};
}

export function ResourceState({
	loading,
	error,
	title = "暂无内容",
	detail,
	onRetry,
	action,
}: {
	loading?: boolean;
	error?: string;
	title?: string;
	detail?: string;
	onRetry?: () => void;
	action?: React.ReactNode;
}) {
	const errorTitle = /rate limit|429/i.test(error ?? "")
		? "请求受限，请稍后重试"
		: /401|未登录|bad credentials/i.test(error ?? "")
			? "登录状态已失效"
			: /403|forbidden/i.test(error ?? "")
				? "访问被拒绝，请检查仓库权限"
				: "加载失败";
	return (
		<div
			className={"resource-state" + (error ? " has-error" : "")}
			role={error ? "alert" : "status"}
		>
			<strong>{loading ? "正在加载…" : error ? errorTitle : title}</strong>
			{(error || detail) && <p>{error || detail}</p>}
			{error && onRetry && (
				<button className="btn" onClick={onRetry}>
					重试
				</button>
			)}
			{!loading && !error && action}
		</div>
	);
}
