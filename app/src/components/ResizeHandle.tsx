export default function ResizeHandle({
	label,
	value,
	min,
	max,
	reverse = false,
	onChange,
}: {
	label: string;
	value: number;
	min: number;
	max: number;
	reverse?: boolean;
	onChange: (n: number) => void;
}) {
	const clamp = (n: number) => Math.min(max, Math.max(min, n));
	return (
		<hr
			className={"resize-handle" + (reverse ? " files-resize" : "")}
			aria-label={label}
			title="左右拖拽调整宽度，也可使用方向键"
			aria-orientation="vertical"
			aria-valuemin={min}
			aria-valuemax={max}
			aria-valuenow={Math.round(value)}
			tabIndex={0}
			onKeyDown={(e) => {
				if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) {
					e.preventDefault();
					onChange(
						e.key === "Home"
							? min
							: e.key === "End"
								? max
								: clamp(
										value +
											(e.key === "ArrowRight" ? 16 : -16) * (reverse ? -1 : 1),
									),
					);
				}
			}}
			onPointerDown={(e) => {
				if (e.button !== 0) return;
				e.preventDefault();
				e.currentTarget.focus();
				e.currentTarget.setPointerCapture(e.pointerId);
				e.currentTarget.dataset.dragging = "true";
				e.currentTarget.dataset.startX = String(e.clientX);
				e.currentTarget.dataset.startValue = String(value);
			}}
			onPointerMove={(e) => {
				if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
				onChange(
					clamp(
						Number(e.currentTarget.dataset.startValue) +
							(e.clientX - Number(e.currentTarget.dataset.startX)) *
								(reverse ? -1 : 1),
					),
				);
			}}
			onPointerUp={(e) => {
				if (e.currentTarget.hasPointerCapture(e.pointerId))
					e.currentTarget.releasePointerCapture(e.pointerId);
			}}
			onLostPointerCapture={(e) => {
				delete e.currentTarget.dataset.dragging;
			}}
		/>
	);
}
