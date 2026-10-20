import { Button } from "@coinbase/cds-web/buttons";
import { TextInput } from "@coinbase/cds-web/controls/TextInput";
import { HStack, VStack } from "@coinbase/cds-web/layout";
import { Text } from "@coinbase/cds-web/typography";

const PRESETS = ["0.1", "0.2", "0.5", "1"];

export function SlippagePicker({
  value,
  custom,
  disabled,
  onChange,
  onCustomChange,
}: {
  value: string;
  custom: string;
  disabled: boolean;
  onChange: (value: string) => void;
  onCustomChange: (value: string) => void;
}) {
  return (
    <VStack gap={1}>
      <Text font="label1">Slippage</Text>
      <HStack role="group" accessibilityLabel="Slippage" gap={1} flexWrap="wrap">
        {[...PRESETS, "Custom"].map((choice) => (
          <Button
            key={choice}
            size="s"
            borderRadius={1000}
            variant={choice === value ? "primary" : "secondary"}
            aria-pressed={choice === value}
            disabled={disabled}
            onClick={() => onChange(choice)}
          >
            {choice === "Custom" ? choice : `${choice}%`}
          </Button>
        ))}
      </HStack>
      {value === "Custom" && (
        <TextInput
          label="Custom slippage (%)"
          inputMode="decimal"
          value={custom}
          disabled={disabled}
          onChange={(event) => onCustomChange(event.target.value)}
        />
      )}
    </VStack>
  );
}
