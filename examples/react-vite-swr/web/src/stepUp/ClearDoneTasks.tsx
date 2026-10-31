import { useState } from "react";
import { retryAfterStepUp, type StepUpRequired, swrKeys } from "react-vite-swr-client/swr";
import { useClearDoneTasksMutation } from "react-vite-swr-client/swr/procedures.hooks";
import { useSWRConfig } from "swr";
import { runtime } from "../runtime.ts";
import { RefusalBanner } from "./RefusalBanner.tsx";
import { StepUpDialog } from "./StepUpDialog.tsx";

type Asking = { stepUp: StepUpRequired; answer: (proved: boolean) => void };

/** "Clear done tasks", through the generated `useClearDoneTasksMutation`.
 *  The procedure declares `@authn(maxAge: 300, amr: ["hwk"])`, so the
 *  demo's password session is refused with a step-up first:
 *  `retryAfterStepUp` reads it as `StepUpRequired`, shows the step-up
 *  dialog, and sends the call once more when the user confirmed. A
 *  declined step-up, or any other refusal, shows the banner keyed on its
 *  code. */
export function ClearDoneTasks({ boardId }: { boardId: number }) {
  const { trigger, isMutating } = useClearDoneTasksMutation(runtime);
  const { mutate } = useSWRConfig();
  const [asking, setAsking] = useState<Asking | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [removed, setRemoved] = useState<number | null>(null);

  // The re-proof `retryAfterStepUp` waits on: it resolves when the user
  // answers the dialog.
  const ask = (stepUp: StepUpRequired) =>
    new Promise<boolean>((resolve) => {
      setAsking({
        stepUp,
        answer: (proved) => {
          setAsking(null);
          resolve(proved);
        },
      });
    });

  async function clear() {
    setError(null);
    setRemoved(null);
    try {
      const result = await retryAfterStepUp(() => trigger({ args: { boardId } }), ask);
      setRemoved(result.removed);
      void mutate(swrKeys.model.Task.listMatches);
    } catch (refusal) {
      setError(refusal);
    }
  }

  return (
    <div>
      <button type="button" onClick={clear} disabled={isMutating || asking !== null}>
        {isMutating ? "Clearing…" : "Clear done tasks"}
      </button>
      {removed !== null && (
        <p className="muted">
          Removed {removed} done task{removed === 1 ? "" : "s"}.
        </p>
      )}
      {error !== null && <RefusalBanner error={error} onRetry={clear} />}
      {asking && <StepUpDialog stepUp={asking.stepUp} onDone={asking.answer} />}
    </div>
  );
}
