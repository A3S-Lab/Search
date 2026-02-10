import { NativeModuleError } from "./errors";

// eslint-disable-next-line @typescript-eslint/no-explicit-any
let nativeModule: any;

try {
  // napi-rs generates this file during build
  nativeModule = require("../index");
} catch (err) {
  throw new NativeModuleError(
    `Failed to load native module. Did you run 'npm run build'? ${err}`
  );
}

export const { JsSearch } = nativeModule;
