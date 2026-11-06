// The demo's stand-in for a signed-in session (cratestack#1245): who the
// user is, when they last authenticated and how (RFC 8176 `amr`). The
// server's `HeaderAuthProvider` (`../../src/auth.rs`) reads them from
// `x-auth-id`, `x-auth-time` and `x-auth-amr` and fills the principal
// contract's `auth_time` and `amr`, which `clearDoneTasks`'s
// `@authn(maxAge: 300, amr: ["hwk"])` checks.
//
// Headers are a stand-in, like the `x-auth-id` this example always sent: a
// real app carries a credential the server verifies, and the re-proof is
// its auth library's ceremony. `examples/step-up-procedures` shows that
// with CrateStack's in-house provider.

export class DemoSession {
  constructor(
    readonly userId: number,
    /** When the user last authenticated, in seconds since the epoch. */
    public authTime: number,
    public amr: readonly string[],
  ) {}

  /** A password sign-in twenty minutes ago: enough for every board and
   *  task operation, too old and too weak for `clearDoneTasks`. */
  static stalePassword(now: number = Date.now()): DemoSession {
    return new DemoSession(1, Math.floor(now / 1000) - 20 * 60, ["pwd"]);
  }

  /** The re-proof: the user confirmed with the device's hardware key, now.
   *  A real app runs its auth library's ceremony here. */
  stepUp(now: number = Date.now()): void {
    this.authTime = Math.floor(now / 1000);
    this.amr = ["hwk", "user"];
  }

  /** The headers every request carries. */
  headers(): Record<string, string> {
    return {
      "x-auth-id": String(this.userId),
      "x-auth-time": String(this.authTime),
      "x-auth-amr": this.amr.join(" "),
    };
  }
}

/** The app's one session. The runtime reads it on every request, so the
 *  call retried after a step-up carries what the step-up screen set. */
export const session = DemoSession.stalePassword();
