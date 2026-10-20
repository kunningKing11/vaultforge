import { Combobox } from "@coinbase/cds-web/alpha/combobox";
import { Select } from "@coinbase/cds-web/alpha/select";
import type { CSSProperties } from "react";

import { SelectionIcon } from "./SelectionIcon";

/** Shared searchable chain/asset control used by Send, Swap, and Transfer USDC. */
export function CdsSelect({
  accessibilityLabel,
  className,
  disabled,
  label,
  onChange,
  options,
  searchable = false,
  style,
  value,
}: {
  accessibilityLabel?: string;
  className?: string;
  disabled?: boolean;
  label?: string;
  onChange: (value: string) => void;
  options: ReadonlyArray<{ label: string; value: string; icon?: string }>;
  searchable?: boolean;
  style?: CSSProperties;
  value: string;
}) {
  const selectedIcon = options.find((option) => option.value === value)?.icon;
  const Control = searchable ? Combobox : Select;
  return (
    <Control
      accessibilityLabel={accessibilityLabel ?? label}
      className={className}
      disabled={disabled}
      label={label}
      onChange={(next) => {
        if (next !== null) onChange(next);
      }}
      options={options.map(({ icon, ...option }) => ({
        ...option,
        media: icon ? <SelectionIcon src={icon} /> : undefined,
      }))}
      startNode={selectedIcon ? <SelectionIcon src={selectedIcon} /> : undefined}
      style={style ?? { width: "100%" }}
      value={value}
    />
  );
}
