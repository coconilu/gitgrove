import * as SelectPrimitive from "@radix-ui/react-select";
import { Check, ChevronDown } from "lucide-react";
import type { ReactNode } from "react";

export interface SelectOption {
	value: string;
	label: ReactNode;
}

interface SelectProps {
	value: string;
	onValueChange: (value: string) => void;
	options: SelectOption[];
	placeholder?: string;
	disabled?: boolean;
	required?: boolean;
	name?: string;
	"aria-label"?: string;
	className?: string;
}

/** 深色主题下拉选择：自定义箭头与选项列表，键盘导航与 ARIA 由 Radix 提供。 */
export function Select({
	value,
	onValueChange,
	options,
	placeholder,
	disabled,
	required,
	name,
	className = "",
	...props
}: SelectProps) {
	return (
		<SelectPrimitive.Root
			value={value || undefined}
			onValueChange={onValueChange}
			disabled={disabled}
			required={required}
			name={name}
		>
			<SelectPrimitive.Trigger
				className={"ui-select-trigger " + className}
				aria-label={props["aria-label"]}
			>
				<SelectPrimitive.Value placeholder={placeholder} />
				<SelectPrimitive.Icon className="ui-select-icon">
					<ChevronDown size={14} aria-hidden="true" />
				</SelectPrimitive.Icon>
			</SelectPrimitive.Trigger>
			<SelectPrimitive.Portal>
				<SelectPrimitive.Content
					className="ui-select-content"
					position="popper"
					sideOffset={4}
				>
					<SelectPrimitive.Viewport className="ui-select-viewport">
						{options.map((o) => (
							<SelectPrimitive.Item
								key={o.value}
								value={o.value}
								className="ui-select-item"
							>
								<SelectPrimitive.ItemIndicator className="ui-select-check">
									<Check size={14} aria-hidden="true" />
								</SelectPrimitive.ItemIndicator>
								<SelectPrimitive.ItemText>{o.label}</SelectPrimitive.ItemText>
							</SelectPrimitive.Item>
						))}
					</SelectPrimitive.Viewport>
				</SelectPrimitive.Content>
			</SelectPrimitive.Portal>
		</SelectPrimitive.Root>
	);
}
