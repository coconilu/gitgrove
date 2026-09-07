import type { LucideIcon } from "lucide-react";
import {
	type ButtonHTMLAttributes,
	useEffect,
	useId,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";

type Props = Omit<
	ButtonHTMLAttributes<HTMLButtonElement>,
	"children" | "aria-label"
> & {
	label: string;
	icon: LucideIcon;
	busy?: boolean;
};

/** Shared tool control: visible keyboard/mouse hint, named icon and native disabled state. */
export default function IconButton({
	label,
	icon: Icon,
	busy = false,
	disabled,
	className = "",
	...props
}: Props) {
	const id = useId();
	const button = useRef<HTMLButtonElement>(null);
	const [hint, setHint] = useState<{
		left: number;
		top: number;
		above: boolean;
		host: Element;
	} | null>(null);
	const showHint = () => {
		const control = button.current;
		if (!control) return;
		const rect = control.getBoundingClientRect();
		const above = rect.bottom + 44 > window.innerHeight;
		setHint({
			left: Math.max(
				96,
				Math.min(window.innerWidth - 96, rect.left + rect.width / 2),
			),
			top: above ? rect.top - 8 : rect.bottom + 8,
			above,
			host: control.closest("dialog") ?? document.body,
		});
	};
	useEffect(() => {
		if (!hint) return;
		const hide = () => setHint(null);
		const dismissOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") hide();
		};
		window.addEventListener("resize", hide);
		document.addEventListener("scroll", hide, true);
		document.addEventListener("keydown", dismissOnEscape);
		return () => {
			window.removeEventListener("resize", hide);
			document.removeEventListener("scroll", hide, true);
			document.removeEventListener("keydown", dismissOnEscape);
		};
	}, [hint]);
	return (
		<>
			<button
				{...props}
				ref={button}
				type={props.type ?? "button"}
				className={"icon-button " + className}
				aria-label={label}
				aria-describedby={hint ? id : undefined}
				aria-busy={busy || undefined}
				disabled={disabled || busy}
				onMouseEnter={showHint}
				onMouseLeave={() => {
					if (document.activeElement !== button.current) setHint(null);
				}}
				onFocus={showHint}
				onBlur={() => setHint(null)}
			>
				<Icon
					size={18}
					strokeWidth={1.75}
					aria-hidden="true"
					focusable="false"
				/>
			</button>
			{hint &&
				createPortal(
					<div
						id={id}
						role="tooltip"
						className="tool-hint"
						style={{
							left: hint.left,
							top: hint.top,
							transform: hint.above
								? "translate(-50%, -100%)"
								: "translateX(-50%)",
						}}
					>
						{busy ? label + "中…" : label}
					</div>,
					hint.host,
				)}
		</>
	);
}
