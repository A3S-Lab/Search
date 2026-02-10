/** Base error class for a3s-search SDK. */
export class A3SSearchError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "A3SSearchError";
  }
}

/** The native module failed to load. */
export class NativeModuleError extends A3SSearchError {
  constructor(message: string) {
    super(message);
    this.name = "NativeModuleError";
  }
}

/** The search operation failed. */
export class SearchError extends A3SSearchError {
  constructor(message: string) {
    super(message);
    this.name = "SearchError";
  }
}
