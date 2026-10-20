import { VStack } from "@coinbase/cds-web/layout";
import { DefaultStepperProgressHorizontal, Stepper } from "@coinbase/cds-web/stepper";
import type { StepperProgressProps } from "@coinbase/cds-web/stepper";
import { Text } from "@coinbase/cds-web/typography";

import type { UniswapOperation } from "../types";

const DIRECT_STEPS = [
  { id: "details", label: "Details" },
  { id: "approve", label: "Approve" },
  { id: "swap", label: "Swap" },
  { id: "done", label: "Done" },
];
const ROUTED_STEPS = [
  ...DIRECT_STEPS.slice(0, 3),
  { id: "receive", label: "Receive" },
  DIRECT_STEPS[3],
];
const SwapStepProgress = (props: StepperProgressProps) => (
  <DefaultStepperProgressHorizontal
    {...props}
    activeFill="bgPrimary"
    completeFill="bgPositive"
    visitedFill="bgPositive"
  />
);

/** Read-only progress; changing the highlighted step never triggers a wallet action. */
export function SwapStepper({
  stage,
  routed,
}: {
  stage: UniswapOperation["stage"] | "details";
  routed: boolean;
}) {
  const steps = routed ? ROUTED_STEPS : DIRECT_STEPS;
  const index = steps.findIndex((step) => step.id === stage);
  return (
    <VStack gap={2} role="group" aria-label="Swap progress">
      <Stepper
        direction="horizontal"
        steps={steps}
        activeStepId={stage}
        complete={stage === "done"}
        StepperProgressComponent={SwapStepProgress}
      />
      <Text font="caption" color="fgMuted" role="status" aria-live="polite">
        Step {index + 1} of {steps.length}: {steps[index]?.label}
      </Text>
    </VStack>
  );
}
