/** A failed gateway request. `status` is 0 for transport failures (no gateway,
 *  DNS, abort) and the HTTP status otherwise. */
export class ApiError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
    this.name = "ApiError";
  }
}
