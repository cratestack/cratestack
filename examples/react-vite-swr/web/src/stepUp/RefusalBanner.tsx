import { INSUFFICIENT_USER_AUTHENTICATION } from "react-vite-swr-client/swr";
import { refusalCode, refusalText } from "./refusal.ts";

/** A banner keyed on the refusal code, with the code shown so a support
 *  request can name it. `onRetry` offers the step-up again. */
export function RefusalBanner({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const code = refusalCode(error);
  return (
    <div className="banner" role="alert" data-refusal={code ?? "unknown"}>
      <p>{refusalText(code)}</p>
      {code && <code>{code}</code>}
      {onRetry && code === INSUFFICIENT_USER_AUTHENTICATION && (
        <button type="button" onClick={onRetry}>
          Confirm now
        </button>
      )}
    </div>
  );
}
