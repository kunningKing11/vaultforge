import { Button } from "@coinbase/cds-web/buttons";
import { TextInput } from "@coinbase/cds-web/controls";
import { VStack } from "@coinbase/cds-web/layout";
import { Text } from "@coinbase/cds-web/typography";
import { type KeyboardEvent, useId, useState } from "react";

type PasswordInputProps = {
  autoComplete?: string;
  label: string;
  minLength?: number;
  onChange: (value: string) => void;
  placeholder?: string;
  required?: boolean;
  value: string;
};

export function PasswordInput({
  autoComplete,
  label,
  minLength,
  onChange,
  placeholder,
  required,
  value,
}: PasswordInputProps) {
  const [visible, setVisible] = useState(false);
  const [capsLockOn, setCapsLockOn] = useState(false);
  const warningId = useId();

  const updateCapsLock = (event: KeyboardEvent<HTMLInputElement>) => {
    setCapsLockOn(event.getModifierState("CapsLock"));
  };

  return (
    <VStack gap={1} width="100%">
      <TextInput
        aria-describedby={capsLockOn ? warningId : undefined}
        autoComplete={autoComplete}
        end={
          <Button
            onClick={() => setVisible((current) => !current)}
            size="s"
            transparent
            type="button"
          >
            {visible ? "Hide" : "Show"}
          </Button>
        }
        label={label}
        minLength={minLength}
        onBlur={() => setCapsLockOn(false)}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={updateCapsLock}
        onKeyUp={updateCapsLock}
        placeholder={placeholder}
        required={required}
        type={visible ? "text" : "password"}
        value={value}
      />
      {capsLockOn && (
        <Text as="p" color="fgWarning" font="label2" id={warningId} role="status">
          Caps Lock is on
        </Text>
      )}
    </VStack>
  );
}
