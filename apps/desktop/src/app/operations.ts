// Every operation the interface can perform, and the versioned IPC call each one is.
//
// # The acceptance criterion this table is
//
// *"Every UI operation maps to a versioned IPC call."* A table is the only form of that sentence a
// test can check, so it is a table. `operations.test.ts` asserts it in both directions:
//
//  - every operation here names a method in `src/ipc/methods.ts` at a version that method exists
//    at — so nothing the interface offers is unimplemented;
//  - every method in that catalogue is reached by at least one operation here — so the surface is
//    no wider than the interface needs it to be, and a method nobody calls cannot sit there
//    looking supported.
//
// # There is no second path
//
// `run` below goes through `DaemonConnection.call` and there is no other way to reach the
// background service from this application. `architecture.test.ts` is what keeps that true: it
// scans every source under `src/` and fails on an import of a storage module, and it fails on any
// file other than `src/ipc/transport.ts` opening a socket.

import type { DaemonConnection } from '../ipc/client.ts';
import { methodEntry } from '../ipc/methods.ts';
import type { WireObject } from '../ipc/protocol.ts';
import { ERROR_COPY } from '../strings/errors.ts';
import { OPERATION_COPY, type OperationCopy } from '../strings/operations.ts';

/** One thing the interface can do, and the call it is. */
export type UiOperation = {
  /** The identifier the interface uses, and the key into {@link OPERATION_COPY}. */
  readonly id: string;
  /** The IPC method this operation is. */
  readonly method: string;
  /** The surface version this operation needs. */
  readonly version: number;
  /** The parameters it always sends. */
  readonly params: WireObject;
  /**
   * Parameters a person or host supplies, when this operation has any.
   *
   * Named here rather than left to the caller so that an operation whose argument is missing is
   * refused at this boundary with a sentence, instead of being sent to the service and coming back
   * as a refusal a person cannot act on.
   */
  readonly inputs?: readonly { readonly name: string; readonly what: string }[];
};

/** Every operation the interface offers. */
export const UI_OPERATIONS: readonly UiOperation[] = [
  { id: 'service.check', method: 'daemon.status', version: 1, params: {} },
  { id: 'service.startup', method: 'startup.report', version: 1, params: {} },
  { id: 'service.describe', method: 'surface.describe', version: 1, params: {} },
  {
    id: 'workspace.open',
    method: 'workspace.open',
    version: 2,
    params: {},
    inputs: [{ name: 'path', what: 'the folder to open' }],
  },
  { id: 'workspace.show', method: 'workspace.state', version: 2, params: {} },
  {
    id: 'review.open',
    method: 'review.open',
    version: 2,
    params: {},
    inputs: [
      { name: 'bundle', what: 'the review bundle' },
      { name: 'target', what: 'the saved change to review' },
      { name: 'opened_by', what: 'the reviewer identity' },
    ],
  },
  {
    id: 'review.open-current',
    method: 'review.open-current',
    version: 6,
    params: {},
    inputs: [{ name: 'opened_by', what: 'the reviewer identity' }],
  },
  {
    id: 'review.approve',
    method: 'review.approve',
    version: 2,
    params: {},
    inputs: [
      { name: 'bundle', what: 'the review bundle' },
      { name: 'target', what: 'the approved saved change' },
      { name: 'receipt', what: 'the signed approval receipt' },
    ],
  },
  { id: 'workspace.follow', method: 'events.subscribe', version: 2, params: {} },
  {
    id: 'folder.import.preview',
    method: 'folder.import.preview',
    version: 3,
    params: {},
    inputs: [{ name: 'source', what: 'the original folder' }],
  },
  {
    id: 'folder.import.confirm',
    method: 'folder.import.confirm',
    version: 3,
    params: {},
    inputs: [
      { name: 'source', what: 'the original folder' },
      { name: 'destination', what: 'the external private workspace location' },
      { name: 'summary', what: 'the exact preview summary' },
    ],
  },
  {
    id: 'folder.import.rollback',
    method: 'folder.import.rollback',
    version: 3,
    params: {},
    inputs: [{ name: 'destination', what: 'the presented working folder' }],
  },
  {
    id: 'workspace.restore.preview',
    method: 'workspace.restore.preview',
    version: 3,
    params: {},
    inputs: [
      { name: 'object', what: 'the file identity' },
      { name: 'target', what: 'the earlier version identity' },
    ],
  },
  {
    id: 'workspace.version.fork',
    method: 'workspace.version.fork',
    version: 5,
    params: {},
    inputs: [
      { name: 'operation', what: 'the durable whole-workspace version' },
      { name: 'destination', what: 'the new external private workspace location' },
    ],
  },
  { id: 'performance.counters', method: 'performance.counters', version: 4, params: {} },
];

/** The operation with this identifier, when there is one. */
export const uiOperation = (id: string): UiOperation | undefined =>
  UI_OPERATIONS.find((operation) => operation.id === id);

/** The words a person reads for this operation. */
export const copyFor = (id: string): OperationCopy | undefined => OPERATION_COPY[id];

/**
 * Perform one operation.
 *
 * Rejects — rather than sending anything — when the identifier names no operation, or when the
 * operation names a method the surface catalogue does not have. Both are programming faults in
 * this application, and failing at the boundary is how they stay visible.
 *
 * Both sentences come from `../strings/errors.ts`. They were written inline here, which put two
 * messages a person can read outside every surface the vocabulary lint scans — the gap
 * `01KZC2SCYPFMTDBTR35V9NXYXN` closed, since an error message is copy exactly as much as a label
 * is.
 */
export const run = async (
  connection: DaemonConnection,
  id: string,
  supplied?: string | WireObject,
): Promise<WireObject> => {
  const operation = uiOperation(id);
  if (operation === undefined) throw new Error(ERROR_COPY.unknownOperation(id));
  if (methodEntry(operation.method) === undefined) {
    throw new Error(ERROR_COPY.methodNotOnSurface(id, operation.method));
  }
  const params = suppliedParams(operation, supplied);
  return connection.call(operation.method, params);
};

/** The parameters this operation needs, or a sentence naming the first one that is missing. */
const suppliedParams = (operation: UiOperation, supplied: string | WireObject | undefined): WireObject => {
  const inputs = operation.inputs ?? [];
  if (inputs.length === 0) return operation.params;
  const values: WireObject =
    typeof supplied === 'string' && inputs.length === 1
      ? { [inputs[0]?.name ?? '']: supplied }
      : typeof supplied === 'object' && supplied !== null
        ? supplied
        : {};
  for (const input of inputs) {
    const value = values[input.name];
    if (typeof value !== 'string' || value.length === 0) {
      throw new Error(ERROR_COPY.argumentMissing(operation.id, input.what));
    }
  }
  return { ...operation.params, ...values };
};
