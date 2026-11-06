import type { StepUpRequired } from "react-vite-swr-client/swr";
import { session } from "../session.ts";

/** The step-up screen: what the refused operation requires, read from the
 *  generated `StepUpRequired`, and the re-proof. `onDone(true)` when the
 *  user confirmed, `onDone(false)` when they declined: what
 *  `retryAfterStepUp`'s `reprove` resolves to. */
export function StepUpDialog({
  stepUp,
  onDone,
}: {
  stepUp: StepUpRequired;
  onDone: (proved: boolean) => void;
}) {
  return (
    <div className="dialog" role="dialog" aria-label="Confirm it is you">
      <h2>Confirm it is you</h2>
      <p>This action needs a fresh sign-in with your security key.</p>
      <ul className="muted">
        {stepUp.maxAge !== undefined && (
          <li>Signed in within the last {Math.floor(stepUp.maxAge / 60)} minutes</li>
        )}
        {stepUp.amr.length > 0 && <li>With: {stepUp.amr.join(", ")}</li>}
        {stepUp.unmet.length > 0 && <li>Missing now: {stepUp.unmet.join(", ")}</li>}
      </ul>
      <div className="row">
        <button
          type="button"
          onClick={() => {
            session.stepUp();
            onDone(true);
          }}
        >
          Confirm with security key
        </button>
        <button type="button" className="link-button" onClick={() => onDone(false)}>
          Not now
        </button>
      </div>
    </div>
  );
}
