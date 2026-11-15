// The error `requireSealed` raises (issue #1371): a call that would have gone
// out unsealed, refused locally instead.
import { CoseFailure } from "@cratestack/ts-types";

/**
 * A call was refused before any `fetch` because `requireSealed` is on and it
 * could not be sealed: its route has no `OP_CONTRACTS` entry, or the `cose`
 * source declined it (`undefined`). `route` names it (the op id, `batch`, or
 * `"<METHOD> <path>"` for a REST call that matched no contract).
 *
 * A {@link CoseFailure} with code `misuse` (a wiring problem on this side, not
 * a rejected answer), so existing `instanceof CoseFailure` handling covers it.
 */
export class SealRequired extends CoseFailure {
  readonly route: string;

  constructor(route: string, reason: "no-contract" | "declined") {
    super(
      "misuse",
      reason === "no-contract"
        ? `requireSealed: no contract for route "${route}"; refusing to send it unsealed`
        : `requireSealed: the cose source declined route "${route}"; refusing to send it unsealed`,
    );
    this.name = "SealRequired";
    this.route = route;
  }
}
